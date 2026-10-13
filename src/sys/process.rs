//! Per-process statistics from /proc and grouping into Apps / Background / System.

use super::apps::{DesktopDb, UnitKind, classify_cgroup};
use crate::util::read;
use std::collections::{HashMap, HashSet};
use std::ffi::CStr;
use std::sync::Arc;
use std::time::Instant;

const PF_KTHREAD: u64 = 0x0020_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    App,
    Background,
    System,
    Kernel,
}

#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    /// Executable file name, e.g. "firefox".
    pub exe_name: Arc<str>,
    /// Friendly name shown on the Processes tab.
    pub name: Arc<str>,
    pub exe_path: Option<Arc<str>>,
    pub cmdline: Arc<str>,
    pub state: char,
    pub uid: u32,
    pub user: Arc<str>,
    pub nice: i32,
    pub threads: u32,
    /// Share of total machine CPU, 0..100.
    pub cpu: f32,
    /// Total CPU time in seconds.
    pub cpu_time: f64,
    /// Seconds after boot.
    pub start_time: f64,
    /// Private memory (anonymous resident pages).
    pub mem: u64,
    pub rss: u64,
    pub shared: u64,
    pub swap: u64,
    pub vsize: u64,
    pub disk_read: f64,
    pub disk_write: f64,
    pub read_total: u64,
    pub write_total: u64,
    pub gpu: f32,
    pub gpu_engine: Option<Arc<str>>,
    pub gpu_mem: u64,
    pub unit: Arc<str>,
    pub class: Class,
    /// Processes with the same group key are shown together.
    pub group: Arc<str>,
    /// Name of the app this process belongs to.
    pub app_name: Option<Arc<str>>,
    pub app_id: Option<Arc<str>>,
    pub icon: Option<Arc<str>>,
}

struct Cached {
    start_ticks: u64,
    comm: Arc<str>,
    exe_name: Arc<str>,
    exe_path: Option<Arc<str>>,
    cmdline: Arc<str>,
    unit: Arc<str>,
    kind: UnitKind,
    cgroup_tick: u64,
    prev_cpu: Option<u64>,
    prev_io: Option<(u64, u64)>,
}

pub struct ProcSampler {
    cache: HashMap<i32, Cached>,
    users: HashMap<u32, (Arc<str>, Arc<str>)>,
    clk_tck: f64,
    ncpu: f64,
    last: Instant,
    tick: u64,
    pub my_uid: u32,
    pub boot_time: u64,
    db: Arc<DesktopDb>,
}

pub struct ProcSample {
    pub procs: Vec<Proc>,
    pub threads: usize,
    /// Per process deltas since the previous sample: (cpu seconds, disk bytes).
    pub deltas: HashMap<i32, (f64, u64)>,
}

impl ProcSampler {
    pub fn new(db: Arc<DesktopDb>) -> Self {
        // SAFETY: sysconf and getuid have no preconditions.
        let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
        let ncpu = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f64;
        let my_uid = unsafe { libc::getuid() };
        let boot_time = read("/proc/stat")
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
            })
            .unwrap_or(0);
        Self {
            cache: HashMap::new(),
            users: HashMap::new(),
            clk_tck,
            ncpu,
            last: Instant::now(),
            tick: 0,
            my_uid,
            boot_time,
            db,
        }
    }

    pub fn users(&self) -> HashMap<u32, (Arc<str>, Arc<str>)> {
        self.users.clone()
    }

    pub fn sample(&mut self) -> ProcSample {
        self.tick += 1;
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().max(0.001);
        self.last = now;
        let uptime: f64 = read("/proc/uptime")
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
            .unwrap_or(0.0);

        let mut procs = Vec::with_capacity(self.cache.len() + 16);
        let mut alive = HashSet::new();
        let mut threads = 0usize;
        let mut deltas = HashMap::new();

        let pids: Vec<i32> = std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect();

        for pid in pids {
            let Some(stat) = read(format!("/proc/{pid}/stat")) else {
                continue;
            };
            let Some(close) = stat.rfind(')') else {
                continue;
            };
            let Some(open) = stat.find('(') else { continue };
            let comm_raw = &stat[open + 1..close];
            let f: Vec<&str> = stat[close + 2..].split_whitespace().collect();
            if f.len() < 22 {
                continue;
            }
            let num = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            let state = f[0].chars().next().unwrap_or('?');
            let ppid = f[1].parse().unwrap_or(0);
            let flags = num(6);
            let ticks = num(11) + num(12);
            let nice = f[16].parse().unwrap_or(0);
            let nthreads = num(17) as u32;
            let start_ticks = num(19);
            let vsize = num(20);
            let kernel = flags & PF_KTHREAD != 0;
            threads += nthreads as usize;
            alive.insert(pid);

            // (Re)initialize the cache entry when the pid is new or reused.
            if self
                .cache
                .get(&pid)
                .is_none_or(|c| c.start_ticks != start_ticks)
            {
                self.cache
                    .insert(pid, new_cached(pid, comm_raw, start_ticks, kernel));
            }
            let c = self.cache.get_mut(&pid).expect("inserted above");
            let start_time = start_ticks as f64 / self.clk_tck;
            let age = uptime - start_time;
            if !kernel && (age < 10.0 || self.tick - c.cgroup_tick >= 30) {
                let (unit, kind) =
                    classify_cgroup(&read(format!("/proc/{pid}/cgroup")).unwrap_or_default());
                c.unit = unit.into();
                c.kind = kind;
                c.cgroup_tick = self.tick;
            }
            if c.comm.as_ref() != comm_raw {
                c.comm = comm_raw.into();
            }

            let cpu = match c.prev_cpu {
                Some(p) => {
                    (ticks.saturating_sub(p) as f64 / (dt * self.clk_tck * self.ncpu) * 100.0)
                        as f32
                }
                None => 0.0,
            };
            let cpu_delta = c
                .prev_cpu
                .map(|p| ticks.saturating_sub(p) as f64 / self.clk_tck);
            c.prev_cpu = Some(ticks);

            let mut uid = 0;
            let (mut mem, mut rss_file, mut rss_shmem, mut swap) = (0, 0, 0, 0);
            if let Some(status) = read(format!("/proc/{pid}/status")) {
                for line in status.lines() {
                    let kb = |l: &str| -> u64 {
                        l.split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0)
                            * 1024
                    };
                    if let Some(rest) = line.strip_prefix("Uid:") {
                        uid = rest
                            .split_whitespace()
                            .next()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                    } else if line.starts_with("RssAnon:") {
                        mem = kb(line);
                    } else if line.starts_with("RssFile:") {
                        rss_file = kb(line);
                    } else if line.starts_with("RssShmem:") {
                        rss_shmem = kb(line);
                    } else if line.starts_with("VmSwap:") {
                        swap = kb(line);
                    }
                }
            }

            let (mut disk_read, mut disk_write, mut read_total, mut write_total) =
                (0.0, 0.0, 0u64, 0u64);
            let mut disk_delta = 0;
            if !kernel && let Some(io) = read(format!("/proc/{pid}/io")) {
                for line in io.lines() {
                    if let Some(v) = line.strip_prefix("read_bytes: ") {
                        read_total = v.trim().parse().unwrap_or(0);
                    } else if let Some(v) = line.strip_prefix("write_bytes: ") {
                        write_total = v.trim().parse().unwrap_or(0);
                    }
                }
                if let Some((pr, pw)) = c.prev_io {
                    let r = read_total.saturating_sub(pr);
                    let w = write_total.saturating_sub(pw);
                    disk_read = r as f64 / dt;
                    disk_write = w as f64 / dt;
                    disk_delta = r + w;
                }
                c.prev_io = Some((read_total, write_total));
            }
            if let Some(cd) = cpu_delta {
                deltas.insert(pid, (cd, disk_delta));
            }

            let user = self
                .users
                .entry(uid)
                .or_insert_with(|| lookup_user(uid))
                .0
                .clone();

            let class = if kernel {
                Class::Kernel
            } else if uid != self.my_uid {
                Class::System
            } else if matches!(c.kind, UnitKind::App(_)) {
                Class::App
            } else {
                Class::Background
            };

            let exe_entry = (!kernel)
                .then(|| {
                    self.db
                        .by_exec(&c.exe_name)
                        .or_else(|| self.db.by_exec(&c.comm))
                })
                .flatten();
            let mut app_name = None;
            let mut app_id = None;
            let mut icon = exe_entry.and_then(|e| e.icon.clone());
            let mut name: Arc<str> = match exe_entry {
                Some(e) if !e.no_display => e.name.clone(),
                _ => c.exe_name.clone(),
            };
            let group: Arc<str> = match &c.kind {
                UnitKind::App(id) if class == Class::App => {
                    let entry = self.db.by_id(id);
                    app_name = Some(
                        entry
                            .map(|e| e.name.clone())
                            .unwrap_or_else(|| id.as_str().into()),
                    );
                    app_id = Some(entry.map(|e| e.id.as_str()).unwrap_or(id).into());
                    if icon.is_none()
                        || exe_entry.is_some_and(|e| Some(&e.id) != entry.map(|x| &x.id))
                    {
                        icon = entry.and_then(|e| e.icon.clone()).or(icon);
                    }
                    c.unit.clone()
                }
                UnitKind::BackgroundApp(id) => {
                    if let Some(e) = self.db.by_id(id) {
                        icon = icon.or_else(|| e.icon.clone());
                        if exe_entry.is_none() {
                            name = e.name.clone();
                        }
                    }
                    c.unit.clone()
                }
                UnitKind::Service => c.unit.clone(),
                _ => format!("pid:{pid}").into(),
            };

            procs.push(Proc {
                pid,
                ppid,
                exe_name: c.exe_name.clone(),
                name,
                exe_path: c.exe_path.clone(),
                cmdline: c.cmdline.clone(),
                state,
                uid,
                user,
                nice,
                threads: nthreads,
                cpu: cpu.clamp(0.0, 100.0),
                cpu_time: ticks as f64 / self.clk_tck,
                start_time,
                mem,
                rss: mem + rss_file + rss_shmem,
                shared: rss_file + rss_shmem,
                swap,
                vsize,
                disk_read,
                disk_write,
                read_total,
                write_total,
                gpu: 0.0,
                gpu_engine: None,
                gpu_mem: 0,
                unit: c.unit.clone(),
                class,
                group,
                app_name,
                app_id,
                icon,
            });
        }
        self.cache.retain(|pid, _| alive.contains(pid));
        promote_untracked_apps(&mut procs, &self.db);
        ProcSample {
            procs,
            threads,
            deltas,
        }
    }
}

/// Without systemd app scopes (or for GUI apps started from a terminal), treat a
/// process whose executable matches a visible desktop entry as the root of an app.
fn promote_untracked_apps(procs: &mut [Proc], db: &DesktopDb) {
    let index: HashMap<i32, usize> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let entry_of = |p: &Proc| {
        if p.class != Class::App && p.class != Class::Background {
            return None;
        }
        db.by_exec(&p.exe_name).filter(|e| !e.no_display)
    };
    // root pid -> (app name, desktop id, icon)
    type AppRef = (Arc<str>, Arc<str>, Option<Arc<str>>);
    let mut roots: HashMap<i32, AppRef> = HashMap::new();
    for p in procs.iter() {
        // Only processes outside a dedicated unit, or inside another app's unit.
        let eligible = p.group.starts_with("pid:") || p.class == Class::App;
        if !eligible {
            continue;
        }
        let Some(e) = entry_of(p) else { continue };
        if p.app_id.as_deref() == Some(e.id.as_str()) {
            continue;
        }
        let parent_same = index
            .get(&p.ppid)
            .and_then(|&i| entry_of(&procs[i]))
            .is_some_and(|pe| pe.id == e.id);
        if !parent_same {
            roots.insert(
                p.pid,
                (e.name.clone(), e.id.as_str().into(), e.icon.clone()),
            );
        }
    }
    if roots.is_empty() {
        return;
    }
    // Assign every process to its nearest promoted ancestor that shares its unit.
    let parents: Vec<(i32, i32, Arc<str>)> = procs
        .iter()
        .map(|p| (p.pid, p.ppid, p.unit.clone()))
        .collect();
    for i in 0..procs.len() {
        let (pid, _, ref unit) = parents[i];
        let mut cur = pid;
        let mut hops = 0;
        let root = loop {
            if roots.contains_key(&cur) {
                break Some(cur);
            }
            let Some(&pi) = index.get(&cur) else {
                break None;
            };
            let ppid = parents[pi].1;
            let Some(&ppi) = index.get(&ppid) else {
                break None;
            };
            if parents[ppi].2 != *unit {
                break None;
            }
            cur = ppid;
            hops += 1;
            if hops > 64 {
                break None;
            }
        };
        if let Some(r) = root {
            let (name, id, icon) = roots[&r].clone();
            let p = &mut procs[i];
            if p.class == Class::Background || p.class == Class::App {
                p.class = Class::App;
                p.group = format!("tree:{r}").into();
                p.app_name = Some(name);
                p.app_id = Some(id);
                if p.icon.is_none() {
                    p.icon = icon;
                }
            }
        }
    }
}

fn new_cached(pid: i32, comm: &str, start_ticks: u64, kernel: bool) -> Cached {
    let comm: Arc<str> = comm.into();
    if kernel {
        return Cached {
            start_ticks,
            exe_name: comm.clone(),
            cmdline: format!("[{comm}]").into(),
            comm,
            exe_path: None,
            unit: "".into(),
            kind: UnitKind::Other,
            cgroup_tick: 0,
            prev_cpu: None,
            prev_io: None,
        };
    }
    let exe_path = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| {
            p.to_string_lossy()
                .trim_end_matches(" (deleted)")
                .to_string()
        });
    let cmd_raw = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let args: Vec<String> = cmd_raw
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let argv0_name = args.first().and_then(|a| {
        let first = a.split(' ').next().unwrap_or(a).trim_end_matches(':');
        std::path::Path::new(first)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
    });
    let exe_name = exe_path
        .as_deref()
        .and_then(|p| {
            std::path::Path::new(p)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        // Versioned binaries such as ".../versions/2.1.296" are better known by their task name.
        .filter(|n| n.chars().any(|c| c.is_alphabetic()))
        .or_else(|| {
            argv0_name.filter(|n| n.starts_with(comm.as_ref()) || comm.starts_with(n.as_str()))
        })
        .unwrap_or_else(|| comm.to_string());
    let cmdline = if args.is_empty() {
        comm.to_string()
    } else {
        args.join(" ")
    };
    Cached {
        start_ticks,
        comm,
        exe_name: exe_name.into(),
        exe_path: exe_path.map(Into::into),
        cmdline: cmdline.into(),
        unit: "".into(),
        kind: UnitKind::Other,
        cgroup_tick: 0,
        prev_cpu: None,
        prev_io: None,
    }
}

fn lookup_user(uid: u32) -> (Arc<str>, Arc<str>) {
    let mut buf = vec![0u8; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the duration of the call; buf is large enough
    // for typical entries (ERANGE simply yields no result).
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        let s: Arc<str> = uid.to_string().into();
        return (s.clone(), s);
    }
    // SAFETY: getpwuid_r succeeded so these point into `buf`.
    let name = unsafe { CStr::from_ptr(pwd.pw_name) }
        .to_string_lossy()
        .to_string();
    let gecos = if pwd.pw_gecos.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pwd.pw_gecos) }
            .to_string_lossy()
            .to_string()
    };
    let full = gecos.split(',').next().unwrap_or("").trim().to_string();
    let full = if full.is_empty() { name.clone() } else { full };
    (name.into(), full.into())
}
