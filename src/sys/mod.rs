//! System data collection. A background thread samples the system at the
//! configured update speed and publishes immutable [`Snapshot`]s.

pub mod app_history;
pub mod apps;
pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod icons;
pub mod memory;
pub mod net;
pub mod pci;
pub mod process;
pub mod report;
pub mod services;
pub mod startup;

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Number of samples kept for graphs ("60 seconds" at normal speed).
pub const HISTORY: usize = 60;

/// Fixed-length history of values, oldest first.
#[derive(Clone, Debug)]
pub struct Series(pub VecDeque<f32>);

impl Default for Series {
    fn default() -> Self {
        Self::new()
    }
}

impl Series {
    pub fn new() -> Self {
        Series(std::iter::repeat_n(0.0, HISTORY).collect())
    }

    pub fn push(&mut self, v: f32) {
        if self.0.len() >= HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }

    pub fn max(&self) -> f32 {
        self.0.iter().copied().fold(0.0, f32::max)
    }
}

#[derive(Default)]
pub struct Snapshot {
    /// Increments with every sample.
    pub seq: u64,
    pub cpu: cpu::CpuStats,
    pub mem: memory::MemStats,
    pub disks: Vec<disk::DiskStats>,
    pub nets: Vec<net::NetStats>,
    pub gpus: Vec<gpu::GpuStats>,
    pub procs: Vec<process::Proc>,
    pub users: HashMap<u32, (Arc<str>, Arc<str>)>,
    pub my_uid: u32,
    pub boot_time: u64,
    /// Overall disk activity 0..100 (busiest disk).
    pub disk_total: f32,
    pub gpu_total: f32,
}

pub enum Cmd {
    SetInterval(Option<Duration>),
    RefreshNow,
    SetHistoryAll(bool),
}

pub struct Shared {
    snapshot: RwLock<Arc<Snapshot>>,
    pub app_history: Mutex<app_history::AppHistory>,
    tx: Mutex<Sender<Cmd>>,
}

impl Shared {
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn send(&self, cmd: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(cmd);
        }
    }
}

/// Start the sampler thread. `interval` of `None` means paused.
pub fn start(ctx: egui::Context, interval: Option<Duration>, history_all: bool) -> Arc<Shared> {
    let (tx, rx) = channel();
    let shared = Arc::new(Shared {
        snapshot: RwLock::new(Arc::new(Snapshot::default())),
        app_history: Mutex::new(app_history::AppHistory::load()),
        tx: Mutex::new(tx),
    });
    let s = shared.clone();
    std::thread::Builder::new()
        .name("sampler".into())
        .spawn(move || run(s, rx, ctx, interval, history_all))
        .expect("spawn sampler thread");
    shared
}

struct Sampler {
    cpu: cpu::CpuSampler,
    mem: memory::MemSampler,
    disk: disk::DiskSampler,
    net: net::NetSampler,
    gpu: gpu::GpuSampler,
    procs: process::ProcSampler,
    seq: u64,
    last: Instant,
}

fn run(
    shared: Arc<Shared>,
    rx: Receiver<Cmd>,
    ctx: egui::Context,
    mut interval: Option<Duration>,
    mut history_all: bool,
) {
    let db = Arc::new(apps::DesktopDb::load());
    let mut sampler = Sampler {
        cpu: cpu::CpuSampler::new(),
        mem: memory::MemSampler::new(),
        disk: disk::DiskSampler::new(),
        net: net::NetSampler::new(),
        gpu: gpu::GpuSampler::new(),
        procs: process::ProcSampler::new(db),
        seq: 0,
        last: Instant::now(),
    };
    // Prime the counters so the first visible sample has real rates.
    sampler.sample(&shared, false);
    std::thread::sleep(Duration::from_millis(250));
    sampler.sample(&shared, false);
    ctx.request_repaint();

    let mut since_save = Instant::now();
    loop {
        let wait = interval.unwrap_or(Duration::from_secs(3600));
        let refresh = match rx.recv_timeout(wait) {
            Ok(Cmd::SetInterval(i)) => {
                interval = i;
                false
            }
            Ok(Cmd::SetHistoryAll(b)) => {
                history_all = b;
                false
            }
            Ok(Cmd::RefreshNow) => true,
            Err(RecvTimeoutError::Timeout) => interval.is_some(),
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if !refresh {
            continue;
        }
        sampler.sample(&shared, history_all);
        ctx.request_repaint();
        if since_save.elapsed() > Duration::from_secs(60) {
            if let Ok(mut h) = shared.app_history.lock() {
                h.save();
            }
            since_save = Instant::now();
        }
    }
}

impl Sampler {
    fn sample(&mut self, shared: &Shared, history_all: bool) {
        let dt = self.last.elapsed().as_secs_f64();
        self.last = Instant::now();
        self.seq += 1;

        let mut cpu = self.cpu.sample();
        let mem = self.mem.sample();
        let disks = self.disk.sample();
        let nets = self.net.sample();
        let mut ps = self.procs.sample();
        let pids: Vec<i32> = ps.procs.iter().map(|p| p.pid).collect();
        let (mut gpus, per_proc_gpu) = self.gpu.sample(&pids);
        gpu::fill_memory_from_procs(&mut gpus, &per_proc_gpu);

        for p in &mut ps.procs {
            if let Some(g) = per_proc_gpu.get(&p.pid) {
                p.gpu = g.util.clamp(0.0, 100.0);
                p.gpu_mem = g.vram + g.shared;
                p.gpu_engine = g.engine.as_ref().map(|e| {
                    let idx = gpus.iter().position(|x| x.info.pdev == g.pdev).unwrap_or(0);
                    Arc::from(format!("GPU {idx} - {e}"))
                });
            }
        }
        cpu.processes = ps
            .procs
            .iter()
            .filter(|p| p.class != process::Class::Kernel)
            .count();
        cpu.threads = ps.threads;

        if dt > 0.0
            && let Ok(mut h) = shared.app_history.lock()
        {
            h.accumulate(&ps.procs, &ps.deltas, dt, history_all);
        }

        let disk_total = disks.iter().map(|d| d.active).fold(0.0, f32::max);
        let gpu_total = gpus.iter().map(|g| g.util).fold(0.0, f32::max);

        let snap = Snapshot {
            seq: self.seq,
            cpu,
            mem,
            disks,
            nets,
            gpus,
            procs: ps.procs,
            users: self.procs.users(),
            my_uid: self.procs.my_uid,
            boot_time: self.procs.boot_time,
            disk_total,
            gpu_total,
        };
        if let Ok(mut s) = shared.snapshot.write() {
            *s = Arc::new(snap);
        }
    }
}
