//! GPU statistics for AMD (amdgpu sysfs), Intel (i915/xe) and NVIDIA (NVML).

pub mod fdinfo;
mod nvidia;

use super::Series;
use crate::util::{read_hex, read_trim, read_u64};
use fdinfo::{FdInfoScanner, ProcGpu, engine_label};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Amd,
    Intel,
    Nvidia,
    Other,
}

#[derive(Debug, Default)]
pub struct GpuInfo {
    pub index: usize,
    pub name: String,
    pub driver: String,
    pub driver_version: String,
    pub pdev: String,
    pub location: String,
    pub vbios: Option<String>,
    pub pcie: Option<String>,
    pub integrated: bool,
}

#[derive(Clone, Default)]
pub struct Engine {
    pub name: String,
    pub util: f32,
    pub history: Series,
}

#[derive(Clone, Default)]
pub struct GpuStats {
    pub info: Arc<GpuInfo>,
    /// Overall utilization 0..100 (the busiest engine, like Windows).
    pub util: f32,
    pub history: Series,
    pub engines: Vec<Engine>,
    pub vram_used: u64,
    pub vram_total: u64,
    pub shared_used: u64,
    pub shared_total: u64,
    pub vram_history: Series,
    pub shared_history: Series,
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
    pub clock_mhz: Option<f32>,
    pub mem_clock_mhz: Option<f32>,
    pub fan_rpm: Option<u32>,
}

struct Card {
    vendor: Vendor,
    device: PathBuf,
    hwmon: Option<PathBuf>,
    /// Engine slot names shown as the four small graphs.
    slots: Vec<&'static str>,
    nvml_index: Option<u32>,
    stats: GpuStats,
}

pub struct GpuSampler {
    cards: Vec<Card>,
    fdinfo: FdInfoScanner,
    nvml: Option<nvidia::Nvidia>,
}

impl GpuSampler {
    pub fn new() -> Self {
        let nvml = nvidia::Nvidia::init();
        let mut cards = enumerate_drm();
        if let Some(nv) = &nvml {
            nv.attach(&mut cards);
        }
        for (i, c) in cards.iter_mut().enumerate() {
            if let Some(info) = Arc::get_mut(&mut c.stats.info) {
                info.index = i;
            }
        }
        Self {
            cards,
            fdinfo: FdInfoScanner::new(),
            nvml,
        }
    }

    pub fn sample(&mut self, pids: &[i32]) -> (Vec<GpuStats>, HashMap<i32, ProcGpu>) {
        let (mut per_proc, engine_util) = if self.cards.iter().any(|c| c.vendor != Vendor::Nvidia) {
            self.fdinfo.scan(pids)
        } else {
            Default::default()
        };

        for card in &mut self.cards {
            let mut engines: HashMap<&'static str, f32> = HashMap::new();
            if let Some(raw) = engine_util.get(&card.stats.info.pdev) {
                for (name, u) in raw {
                    let label = engine_label(name);
                    let slot = slot_for(&card.slots, &label);
                    if let Some(slot) = slot {
                        let e = engines.entry(slot).or_default();
                        *e = e.max(*u);
                    }
                }
            }
            let s = &mut card.stats;
            match card.vendor {
                Vendor::Amd => sample_amd(
                    card.device.as_path(),
                    card.hwmon.as_deref(),
                    s,
                    &mut engines,
                ),
                Vendor::Intel => sample_intel(card.device.as_path(), card.hwmon.as_deref(), s),
                Vendor::Nvidia | Vendor::Other => {}
            }
            if let (Some(nv), Some(idx)) = (&mut self.nvml, card.nvml_index) {
                nv.sample(idx, s, &mut engines, &mut per_proc);
            }

            let mut overall: f32 = engines.values().copied().fold(0.0, f32::max);
            if card.vendor == Vendor::Amd {
                overall =
                    overall.max(read_u64(card.device.join("gpu_busy_percent")).unwrap_or(0) as f32);
            }
            if card.vendor == Vendor::Nvidia {
                overall = overall.max(s.util);
            }
            s.util = overall.clamp(0.0, 100.0);
            s.history.push(s.util);
            if s.engines.len() != card.slots.len() {
                s.engines = card
                    .slots
                    .iter()
                    .map(|n| Engine {
                        name: n.to_string(),
                        ..Default::default()
                    })
                    .collect();
            }
            for e in &mut s.engines {
                e.util = engines
                    .get(e.name.as_str())
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 100.0);
                e.history.push(e.util);
            }
            s.vram_history.push(s.vram_used as f32);
            s.shared_history.push(s.shared_used as f32);
        }
        (
            self.cards.iter().map(|c| c.stats.clone()).collect(),
            per_proc,
        )
    }
}

fn slot_for(slots: &[&'static str], label: &str) -> Option<&'static str> {
    if let Some(s) = slots.iter().find(|s| **s == label) {
        return Some(s);
    }
    // Fold the video engines into a combined slot when the card only has one.
    if matches!(
        label,
        "Video Decode" | "Video Encode" | "Video Codec" | "JPEG"
    ) {
        return slots.iter().find(|s| s.starts_with("Video")).copied();
    }
    None
}

fn enumerate_drm() -> Vec<Card> {
    let mut names: Vec<(u32, PathBuf)> = std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let num: u32 = n.strip_prefix("card")?.parse().ok()?;
            Some((num, e.path()))
        })
        .collect();
    names.sort();

    let mut cards = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, path) in names {
        let Ok(device) = path.join("device").canonicalize() else {
            continue;
        };
        let Some(vendor_id) = read_hex(device.join("vendor")) else {
            continue;
        };
        let pdev = device
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        if !seen.insert(pdev.clone()) {
            continue;
        }
        let vendor = match vendor_id {
            0x1002 => Vendor::Amd,
            0x8086 => Vendor::Intel,
            0x10de => Vendor::Nvidia,
            _ => Vendor::Other,
        };
        let driver = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
            .unwrap_or_default();
        let name = super::pci::name_for_sysfs_device(&device).unwrap_or_else(|| "GPU".into());
        let hwmon = std::fs::read_dir(device.join("hwmon"))
            .into_iter()
            .flatten()
            .flatten()
            .next()
            .map(|e| e.path());
        let kernel = read_trim("/proc/sys/kernel/osrelease").unwrap_or_default();
        let driver_version = read_trim(format!("/sys/module/{driver}/version"))
            .unwrap_or_else(|| format!("Linux {kernel}"));
        let pcie = match (
            read_trim(device.join("current_link_speed")),
            read_trim(device.join("current_link_width")),
        ) {
            (Some(s), Some(w)) => Some(format!("{s} x{w}")),
            _ => None,
        };
        // Integrated GPUs sit on bus 0.
        let integrated = pdev.split(':').nth(1) == Some("00");
        let slots: Vec<&'static str> = match vendor {
            Vendor::Amd => vec!["3D", "Compute", "Copy", "Video Codec"],
            Vendor::Intel => vec!["3D", "Copy", "Video Decode", "Video Processing"],
            Vendor::Nvidia => vec!["3D", "Memory", "Video Encode", "Video Decode"],
            Vendor::Other => vec!["3D", "Compute", "Copy", "Video Decode"],
        };
        cards.push(Card {
            vendor,
            hwmon,
            slots,
            nvml_index: None,
            stats: GpuStats {
                info: Arc::new(GpuInfo {
                    index: 0,
                    name,
                    driver,
                    driver_version,
                    location: pci_location(&pdev),
                    pdev,
                    vbios: read_trim(device.join("vbios_version")),
                    pcie,
                    integrated,
                }),
                ..Default::default()
            },
            device,
        });
    }
    // Discrete GPUs first, like "GPU 0" on Windows usually being the main card.
    cards.sort_by_key(|c| c.stats.info.integrated);
    cards
}

pub(super) fn pci_location(pdev: &str) -> String {
    // "0000:03:00.0" -> PCI bus 3, device 0, function 0
    let parts: Vec<&str> = pdev.split([':', '.']).collect();
    if parts.len() == 4 {
        let n = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
        format!(
            "PCI bus {}, device {}, function {}",
            n(parts[1]),
            n(parts[2]),
            n(parts[3])
        )
    } else {
        pdev.to_string()
    }
}

fn hwmon_f(hwmon: Option<&Path>, file: &str, div: f32) -> Option<f32> {
    read_u64(hwmon?.join(file)).map(|v| v as f32 / div)
}

fn sample_amd(
    dev: &Path,
    hwmon: Option<&Path>,
    s: &mut GpuStats,
    engines: &mut HashMap<&'static str, f32>,
) {
    s.vram_used = read_u64(dev.join("mem_info_vram_used")).unwrap_or(0);
    s.vram_total = read_u64(dev.join("mem_info_vram_total")).unwrap_or(0);
    s.shared_used = read_u64(dev.join("mem_info_gtt_used")).unwrap_or(0);
    s.shared_total = read_u64(dev.join("mem_info_gtt_total")).unwrap_or(0);
    if let Some(v) = read_u64(dev.join("vcn_busy_percent")) {
        let e = engines.entry("Video Codec").or_default();
        *e = e.max(v as f32);
    }
    s.temp_c = hwmon_f(hwmon, "temp1_input", 1000.0);
    s.power_w =
        hwmon_f(hwmon, "power1_average", 1e6).or_else(|| hwmon_f(hwmon, "power1_input", 1e6));
    s.clock_mhz = hwmon_f(hwmon, "freq1_input", 1e6);
    s.mem_clock_mhz = hwmon_f(hwmon, "freq2_input", 1e6);
    s.fan_rpm = hwmon
        .and_then(|h| read_u64(h.join("fan1_input")))
        .map(|v| v as u32);
}

fn sample_intel(dev: &Path, hwmon: Option<&Path>, s: &mut GpuStats) {
    let drm = dev.join("drm");
    let card = std::fs::read_dir(&drm)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with("card"))
        });
    if let Some(card) = card {
        s.clock_mhz = read_u64(card.join("gt_act_freq_mhz"))
            .or_else(|| read_u64(card.join("gt_cur_freq_mhz")))
            .map(|v| v as f32);
    }
    if s.clock_mhz.is_none() {
        s.clock_mhz = read_u64(dev.join("tile0/gt0/freq0/act_freq")).map(|v| v as f32);
    }
    // Integrated graphics share system memory; Windows reports half of RAM as shared.
    if s.shared_total == 0 {
        let mem = crate::util::read("/proc/meminfo").unwrap_or_default();
        let total_kb: u64 = mem
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
            .unwrap_or(0);
        s.shared_total = total_kb * 1024 / 2;
    }
    s.temp_c = hwmon_f(hwmon, "temp1_input", 1000.0);
    s.power_w = hwmon_f(hwmon, "power1_input", 1e6);
}

/// Sum per-process GPU memory into the device totals for drivers without global counters.
pub fn fill_memory_from_procs(gpus: &mut [GpuStats], procs: &HashMap<i32, ProcGpu>) {
    for g in gpus.iter_mut() {
        if g.vram_total > 0 || g.info.driver == "amdgpu" || g.info.driver == "nvidia" {
            continue;
        }
        let shared: u64 = procs
            .values()
            .filter(|p| p.pdev == g.info.pdev)
            .map(|p| p.shared + p.vram)
            .sum();
        g.shared_used = shared;
        if let Some(last) = g.shared_history.0.back_mut() {
            *last = shared as f32;
        }
    }
}
