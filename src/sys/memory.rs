//! Memory statistics from /proc/meminfo.

use super::Series;
use crate::util::{read, read_u64};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct MemStats {
    /// Memory usable by the kernel (MemTotal).
    pub total: u64,
    /// Physically installed memory, if it could be determined.
    pub installed: Option<u64>,
    pub available: u64,
    pub free: u64,
    pub used: u64,
    pub cached: u64,
    pub buffers: u64,
    pub shared: u64,
    pub modified: u64,
    pub standby: u64,
    pub slab_reclaimable: u64,
    pub slab_unreclaimable: u64,
    pub page_tables: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Data held compressed in zswap.
    pub compressed: u64,
    /// Utilization 0..100.
    pub usage: f32,
    pub history: Series,
}

pub struct MemSampler {
    history: Series,
    installed: Option<u64>,
}

impl MemSampler {
    pub fn new() -> Self {
        Self {
            history: Series::new(),
            installed: installed_memory(),
        }
    }

    pub fn sample(&mut self) -> MemStats {
        let text = read("/proc/meminfo").unwrap_or_default();
        let m: HashMap<&str, u64> = text
            .lines()
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                let n: u64 = v.split_whitespace().next()?.parse().ok()?;
                Some((k, n * 1024))
            })
            .collect();
        let g = |k: &str| m.get(k).copied().unwrap_or(0);

        let total = g("MemTotal");
        let free = g("MemFree");
        let available = if m.contains_key("MemAvailable") {
            g("MemAvailable")
        } else {
            free + g("Cached") + g("Buffers")
        };
        let used = total.saturating_sub(available);
        let modified = g("Dirty") + g("Writeback");
        let standby = available.saturating_sub(free).saturating_sub(modified);
        let usage = if total > 0 {
            used as f32 / total as f32 * 100.0
        } else {
            0.0
        };
        self.history.push(usage);

        MemStats {
            total,
            installed: self.installed.filter(|&i| i >= total),
            available,
            free,
            used,
            cached: g("Cached") + g("SReclaimable"),
            buffers: g("Buffers"),
            shared: g("Shmem"),
            modified,
            standby,
            slab_reclaimable: g("SReclaimable"),
            slab_unreclaimable: g("SUnreclaim"),
            page_tables: g("PageTables"),
            committed: g("Committed_AS"),
            commit_limit: g("CommitLimit"),
            swap_total: g("SwapTotal"),
            swap_used: g("SwapTotal").saturating_sub(g("SwapFree")),
            compressed: g("Zswapped"),
            usage,
            history: self.history.clone(),
        }
    }
}

/// Installed RAM from the memory hotplug blocks in sysfs.
fn installed_memory() -> Option<u64> {
    let block = read("/sys/devices/system/memory/block_size_bytes")
        .and_then(|s| u64::from_str_radix(s.trim(), 16).ok())?;
    let mut online = 0u64;
    for e in std::fs::read_dir("/sys/devices/system/memory")
        .ok()?
        .flatten()
    {
        if e.file_name().to_string_lossy().starts_with("memory")
            && read_u64(e.path().join("online")) == Some(1)
        {
            online += 1;
        }
    }
    (online > 0).then_some(online * block)
}
