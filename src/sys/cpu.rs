//! CPU utilization, frequency and static topology information.

use super::Series;
use crate::util::{read, read_trim, read_u64};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct CpuInfo {
    pub model: String,
    pub base_mhz: Option<f64>,
    pub sockets: usize,
    pub cores: usize,
    pub logical: usize,
    pub virtualization: String,
    pub l1: u64,
    pub l2: u64,
    pub l3: u64,
}

#[derive(Clone, Default)]
pub struct CpuStats {
    pub info: Arc<CpuInfo>,
    /// Total utilization 0..100.
    pub usage: f32,
    pub speed_mhz: f64,
    pub history: Series,
    pub kernel_history: Series,
    pub per_cpu_history: Vec<Series>,
    pub processes: usize,
    pub threads: usize,
    pub handles: u64,
    pub uptime: f64,
}

#[derive(Clone, Copy, Default)]
struct Times {
    busy: u64,
    kernel: u64,
    total: u64,
}

pub struct CpuSampler {
    info: Arc<CpuInfo>,
    prev: Times,
    prev_per: Vec<Times>,
    history: Series,
    kernel_history: Series,
    per_cpu_history: Vec<Series>,
}

impl CpuSampler {
    pub fn new() -> Self {
        let info = Arc::new(load_info());
        let n = info.logical;
        Self {
            info,
            prev: Times::default(),
            prev_per: vec![Times::default(); n],
            history: Series::new(),
            kernel_history: Series::new(),
            per_cpu_history: vec![Series::new(); n],
        }
    }

    pub fn sample(&mut self) -> CpuStats {
        let stat = read("/proc/stat").unwrap_or_default();
        let mut usage = 0.0;
        let mut kernel = 0.0;
        let mut per_cpu = vec![0.0; self.prev_per.len()];
        for line in stat.lines() {
            let Some(rest) = line.strip_prefix("cpu") else {
                continue;
            };
            // "cpu  1 2 3 ..." is the total, "cpu7 1 2 3 ..." a single core.
            let Some((label, values)) = rest.split_once(' ') else {
                continue;
            };
            let v: Vec<u64> = values
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            if v.len() < 8 {
                continue;
            }
            // user nice system idle iowait irq softirq steal
            let total: u64 = v[..8].iter().sum();
            let idle = v[3] + v[4];
            let t = Times {
                busy: total - idle,
                kernel: v[2] + v[5] + v[6],
                total,
            };
            if label.is_empty() {
                (usage, kernel) = percent(&self.prev, &t);
                self.prev = t;
            } else if let Ok(i) = label.parse::<usize>() {
                if i >= self.prev_per.len() {
                    self.prev_per.resize(i + 1, Times::default());
                    self.per_cpu_history.resize(i + 1, Series::new());
                    per_cpu.resize(i + 1, 0.0);
                }
                per_cpu[i] = percent(&self.prev_per[i], &t).0;
                self.prev_per[i] = t;
            }
        }
        self.history.push(usage);
        self.kernel_history.push(kernel);
        for (h, v) in self.per_cpu_history.iter_mut().zip(&per_cpu) {
            h.push(*v);
        }

        let uptime = read("/proc/uptime")
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
            .unwrap_or(0.0);
        let handles = read("/proc/sys/fs/file-nr")
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
            .unwrap_or(0);

        CpuStats {
            info: self.info.clone(),
            usage,
            speed_mhz: current_speed_mhz(self.prev_per.len()),
            history: self.history.clone(),
            kernel_history: self.kernel_history.clone(),
            per_cpu_history: self.per_cpu_history.clone(),
            processes: 0,
            threads: 0,
            handles,
            uptime,
        }
    }
}

fn percent(prev: &Times, now: &Times) -> (f32, f32) {
    let dt = now.total.saturating_sub(prev.total);
    if dt == 0 || prev.total == 0 {
        return (0.0, 0.0);
    }
    let busy = now.busy.saturating_sub(prev.busy) as f32 / dt as f32 * 100.0;
    let kernel = now.kernel.saturating_sub(prev.kernel) as f32 / dt as f32 * 100.0;
    (busy.clamp(0.0, 100.0), kernel.clamp(0.0, 100.0))
}

fn current_speed_mhz(n: usize) -> f64 {
    let mut sum = 0u64;
    let mut count = 0u64;
    for i in 0..n {
        if let Some(khz) = read_u64(format!(
            "/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"
        )) {
            sum += khz;
            count += 1;
        }
    }
    if count > 0 {
        return sum as f64 / count as f64 / 1000.0;
    }
    // Fall back to /proc/cpuinfo.
    let info = read("/proc/cpuinfo").unwrap_or_default();
    let mhz: Vec<f64> = info
        .lines()
        .filter(|l| l.starts_with("cpu MHz"))
        .filter_map(|l| l.split(':').nth(1)?.trim().parse().ok())
        .collect();
    if mhz.is_empty() {
        0.0
    } else {
        mhz.iter().sum::<f64>() / mhz.len() as f64
    }
}

fn load_info() -> CpuInfo {
    let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
    let field = |name: &str| {
        cpuinfo
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
    };
    let model = field("model name")
        .or_else(|| field("Model"))
        .or_else(|| field("Hardware"))
        .unwrap_or_else(|| "Processor".into());
    let flags = field("flags").unwrap_or_default();
    let virt_capable = flags.split(' ').any(|f| f == "vmx" || f == "svm");
    let virtualization = if !virt_capable {
        "Not supported"
    } else if Path::new("/dev/kvm").exists() {
        "Enabled"
    } else {
        "Disabled"
    }
    .to_string();

    let base = Path::new("/sys/devices/system/cpu/cpu0");
    let base_mhz = read_u64(base.join("cpufreq/base_frequency"))
        .map(|khz| khz as f64 / 1000.0)
        .or_else(|| read_u64(base.join("acpi_cppc/nominal_freq")).map(|m| m as f64))
        .or_else(|| read_u64(base.join("cpufreq/cpuinfo_max_freq")).map(|k| k as f64 / 1000.0));

    let mut packages = BTreeSet::new();
    let mut cores = BTreeSet::new();
    let mut logical = 0;
    let mut caches = BTreeSet::new();
    let (mut l1, mut l2, mut l3) = (0, 0, 0);
    for entry in std::fs::read_dir("/sys/devices/system/cpu")
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(num) = name.strip_prefix("cpu") else {
            continue;
        };
        if num.parse::<usize>().is_err() {
            continue;
        }
        let topo = entry.path().join("topology");
        if !topo.exists() {
            continue;
        }
        logical += 1;
        if let Some(p) = read_trim(topo.join("physical_package_id")) {
            packages.insert(p);
        }
        if let Some(c) = read_trim(topo.join("core_cpus_list"))
            .or_else(|| read_trim(topo.join("thread_siblings_list")))
        {
            cores.insert(c);
        }
        for idx in std::fs::read_dir(entry.path().join("cache"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = idx.path();
            let (Some(level), Some(kind), Some(size), Some(shared)) = (
                read_trim(p.join("level")),
                read_trim(p.join("type")),
                read_trim(p.join("size")),
                read_trim(p.join("shared_cpu_list")),
            ) else {
                continue;
            };
            if !caches.insert((level.clone(), kind, shared)) {
                continue;
            }
            let bytes = parse_cache_size(&size);
            match level.as_str() {
                "1" => l1 += bytes,
                "2" => l2 += bytes,
                "3" => l3 += bytes,
                _ => {}
            }
        }
    }
    let logical = logical.max(1);
    CpuInfo {
        model,
        base_mhz,
        sockets: packages.len().max(1),
        cores: if cores.is_empty() {
            logical
        } else {
            cores.len()
        },
        logical,
        virtualization,
        l1,
        l2,
        l3,
    }
}

fn parse_cache_size(s: &str) -> u64 {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix('K') {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024 * 1024)
    } else {
        (s, 1)
    };
    num.parse::<u64>().unwrap_or(0) * mult
}
