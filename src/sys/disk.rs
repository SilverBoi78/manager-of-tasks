//! Physical disk activity from /proc/diskstats.

use super::Series;
use crate::util::{read, read_trim, read_u64};
use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct DiskInfo {
    pub index: usize,
    pub name: String,
    pub model: String,
    pub kind: String,
    pub capacity: u64,
    pub formatted: u64,
    pub system_disk: bool,
    pub page_file: bool,
    pub mounts: Vec<String>,
}

#[derive(Clone, Default)]
pub struct DiskStats {
    pub info: Arc<DiskInfo>,
    /// Active time 0..100.
    pub active: f32,
    pub read_bps: f64,
    pub write_bps: f64,
    pub avg_response_ms: f64,
    pub active_history: Series,
    pub read_history: Series,
    pub write_history: Series,
}

#[derive(Clone, Copy, Default)]
struct Counters {
    reads: u64,
    sectors_read: u64,
    ms_reading: u64,
    writes: u64,
    sectors_written: u64,
    ms_writing: u64,
    io_ticks: u64,
}

struct Entry {
    stats: DiskStats,
    prev: Option<Counters>,
}

pub struct DiskSampler {
    entries: Vec<Entry>,
    last: Instant,
    tick: u64,
}

impl DiskSampler {
    pub fn new() -> Self {
        let mut s = Self {
            entries: Vec::new(),
            last: Instant::now(),
            tick: 0,
        };
        s.refresh_devices();
        s
    }

    fn refresh_devices(&mut self) {
        let names = list_disks();
        let mounts = read_mounts();
        let swaps = read_swaps();
        let mut entries = Vec::new();
        for (index, name) in names.into_iter().enumerate() {
            let info = Arc::new(disk_info(index, &name, &mounts, &swaps));
            let existing = self
                .entries
                .iter()
                .position(|e| e.stats.info.name == name)
                .map(|i| self.entries.swap_remove(i));
            let entry = match existing {
                Some(mut e) => {
                    e.stats.info = info;
                    e
                }
                None => Entry {
                    stats: DiskStats {
                        info,
                        ..Default::default()
                    },
                    prev: None,
                },
            };
            entries.push(entry);
        }
        self.entries = entries;
    }

    pub fn sample(&mut self) -> Vec<DiskStats> {
        self.tick += 1;
        if self.tick.is_multiple_of(10) {
            self.refresh_devices();
        }
        let now = Instant::now();
        let dt_ms = (now - self.last).as_secs_f64() * 1000.0;
        self.last = now;

        let text = read("/proc/diskstats").unwrap_or_default();
        let mut counters: HashMap<&str, Counters> = HashMap::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 14 {
                continue;
            }
            let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            counters.insert(
                f[2],
                Counters {
                    reads: n(3),
                    sectors_read: n(5),
                    ms_reading: n(6),
                    writes: n(7),
                    sectors_written: n(9),
                    ms_writing: n(10),
                    io_ticks: n(12),
                },
            );
        }

        for e in &mut self.entries {
            let Some(c) = counters.get(e.stats.info.name.as_str()).copied() else {
                continue;
            };
            let s = &mut e.stats;
            if let (Some(p), true) = (e.prev, dt_ms > 0.0) {
                s.active = ((c.io_ticks.saturating_sub(p.io_ticks)) as f64 / dt_ms * 100.0)
                    .clamp(0.0, 100.0) as f32;
                s.read_bps =
                    c.sectors_read.saturating_sub(p.sectors_read) as f64 * 512.0 / dt_ms * 1000.0;
                s.write_bps = c.sectors_written.saturating_sub(p.sectors_written) as f64 * 512.0
                    / dt_ms
                    * 1000.0;
                let ios = (c.reads + c.writes).saturating_sub(p.reads + p.writes);
                let ms = (c.ms_reading + c.ms_writing).saturating_sub(p.ms_reading + p.ms_writing);
                s.avg_response_ms = if ios > 0 { ms as f64 / ios as f64 } else { 0.0 };
            }
            e.prev = Some(c);
            s.active_history.push(s.active);
            s.read_history.push(s.read_bps as f32);
            s.write_history.push(s.write_bps as f32);
        }
        self.entries.iter().map(|e| e.stats.clone()).collect()
    }
}

fn list_disks() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("/sys/block")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            !["loop", "ram", "zram", "dm-", "sr", "fd", "nbd"]
                .iter()
                .any(|p| n.starts_with(p))
        })
        .filter(|n| read_u64(format!("/sys/block/{n}/size")).unwrap_or(0) > 0)
        .collect();
    names.sort_by_key(|a| natural_key(a));
    names
}

fn natural_key(s: &str) -> (String, u64) {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let letters: String = s.chars().filter(|c| !c.is_ascii_digit()).collect();
    (letters, digits.parse().unwrap_or(0))
}

struct Mount {
    source: String,
    target: String,
}

fn read_mounts() -> Vec<Mount> {
    let text = read("/proc/self/mountinfo").unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let target = left.split(' ').nth(4)?.replace("\\040", " ");
            let source = right.split(' ').nth(1)?.to_string();
            source
                .starts_with("/dev/")
                .then_some(Mount { source, target })
        })
        .collect()
}

fn read_swaps() -> Vec<String> {
    read("/proc/swaps")
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let file = it.next()?;
            (it.next()? == "partition").then(|| file.to_string())
        })
        .collect()
}

/// Map a block device (partition, dm device, ...) to the physical disks backing it.
pub fn backing_disks(dev: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let name = Path::new(dev)
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        .unwrap_or_else(|| dev.trim_start_matches("/dev/").to_string());
    resolve(&name, &mut out, 0);
    out
}

fn resolve(name: &str, out: &mut BTreeSet<String>, depth: u32) {
    if depth > 8 {
        return;
    }
    let class = PathBuf::from("/sys/class/block").join(name);
    if class.join("partition").exists() {
        if let Some(parent) = class
            .canonicalize()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        {
            resolve(&parent, out, depth + 1);
        }
        return;
    }
    let slaves: Vec<String> = std::fs::read_dir(class.join("slaves"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    if slaves.is_empty() {
        out.insert(name.to_string());
    } else {
        for s in slaves {
            resolve(&s, out, depth + 1);
        }
    }
}

fn statvfs_size(path: &str) -> u64 {
    let Ok(c) = CString::new(path) else { return 0 };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` is a valid out pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
        st.f_blocks as u64 * st.f_frsize as u64
    } else {
        0
    }
}

fn disk_info(index: usize, name: &str, mounts: &[Mount], swaps: &[String]) -> DiskInfo {
    let base = PathBuf::from("/sys/block").join(name);
    let model = read_trim(base.join("device/model"))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let vendor = read_trim(base.join("device/vendor")).unwrap_or_default();
            (!vendor.is_empty()).then_some(vendor)
        })
        .unwrap_or_else(|| name.to_string());
    let rotational = read_u64(base.join("queue/rotational")) == Some(1);
    let removable = read_u64(base.join("removable")) == Some(1)
        || base
            .canonicalize()
            .map(|p| p.to_string_lossy().contains("/usb"))
            .unwrap_or(false);
    let kind = if removable {
        "Removable"
    } else if name.starts_with("nvme") {
        "SSD (NVMe)"
    } else if !rotational {
        "SSD"
    } else {
        "HDD"
    }
    .to_string();

    let mut seen_sources = BTreeSet::new();
    let mut formatted = 0;
    let mut targets = Vec::new();
    let mut system_disk = false;
    for m in mounts {
        if !backing_disks(&m.source).contains(name) {
            continue;
        }
        if m.target == "/" {
            system_disk = true;
        }
        targets.push(m.target.clone());
        if seen_sources.insert(m.source.clone()) {
            formatted += statvfs_size(&m.target);
        }
    }
    targets.sort_by_key(|t| t.len());
    targets.truncate(6);
    let page_file = swaps.iter().any(|s| backing_disks(s).contains(name));

    DiskInfo {
        index,
        name: name.to_string(),
        model,
        kind,
        capacity: read_u64(base.join("size")).unwrap_or(0) * 512,
        formatted,
        system_disk,
        page_file,
        mounts: targets,
    }
}
