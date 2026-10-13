//! NVIDIA GPUs through NVML. libnvidia-ml.so is loaded at runtime, so the
//! application works fine on machines without the NVIDIA driver.

use super::fdinfo::ProcGpu;
use super::{Card, GpuInfo, GpuStats, Vendor, pci_location};
use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Nvidia {
    nvml: Nvml,
    last_ts: HashMap<u32, u64>,
}

fn normalize_bus_id(id: &str) -> String {
    // NVML: "00000000:01:00.0", sysfs: "0000:01:00.0".
    let id = id.to_lowercase();
    let n = id.len();
    if n > 12 { id[n - 12..].to_string() } else { id }
}

impl Nvidia {
    pub fn init() -> Option<Self> {
        let nvml = Nvml::init().ok()?;
        Some(Self {
            nvml,
            last_ts: HashMap::new(),
        })
    }

    pub(super) fn attach(&self, cards: &mut Vec<Card>) {
        let driver_version = self.nvml.sys_driver_version().unwrap_or_default();
        let count = self.nvml.device_count().unwrap_or(0);
        for i in 0..count {
            let Ok(dev) = self.nvml.device_by_index(i) else {
                continue;
            };
            let name = dev.name().unwrap_or_else(|_| "NVIDIA GPU".into());
            let bus = dev
                .pci_info()
                .map(|p| normalize_bus_id(&p.bus_id))
                .unwrap_or_default();
            let info = |old: Option<&GpuInfo>| GpuInfo {
                index: 0,
                name: name.clone(),
                driver: "nvidia".into(),
                driver_version: driver_version.clone(),
                location: pci_location(&bus),
                pdev: bus.clone(),
                vbios: dev.vbios_version().ok(),
                pcie: old.and_then(|o| o.pcie.clone()),
                integrated: false,
            };
            if let Some(card) = cards.iter_mut().find(|c| c.stats.info.pdev == bus) {
                card.nvml_index = Some(i);
                card.stats.info = Arc::new(info(Some(&card.stats.info)));
            } else {
                cards.push(Card {
                    vendor: Vendor::Nvidia,
                    device: std::path::PathBuf::from(format!("/sys/bus/pci/devices/{bus}")),
                    hwmon: None,
                    slots: vec!["3D", "Memory", "Video Encode", "Video Decode"],
                    nvml_index: Some(i),
                    stats: GpuStats {
                        info: Arc::new(info(None)),
                        ..Default::default()
                    },
                });
            }
        }
    }

    pub(super) fn sample(
        &mut self,
        idx: u32,
        s: &mut GpuStats,
        engines: &mut HashMap<&'static str, f32>,
        per_proc: &mut HashMap<i32, ProcGpu>,
    ) {
        let Ok(dev) = self.nvml.device_by_index(idx) else {
            return;
        };
        if let Ok(u) = dev.utilization_rates() {
            s.util = u.gpu as f32;
            engines.insert("3D", u.gpu as f32);
            engines.insert("Memory", u.memory as f32);
        }
        if let Ok(e) = dev.encoder_utilization() {
            engines.insert("Video Encode", e.utilization as f32);
        }
        if let Ok(d) = dev.decoder_utilization() {
            engines.insert("Video Decode", d.utilization as f32);
        }
        if let Ok(m) = dev.memory_info() {
            s.vram_used = m.used;
            s.vram_total = m.total;
        }
        s.temp_c = dev
            .temperature(TemperatureSensor::Gpu)
            .ok()
            .map(|t| t as f32);
        s.power_w = dev.power_usage().ok().map(|mw| mw as f32 / 1000.0);
        s.clock_mhz = dev.clock_info(Clock::Graphics).ok().map(|c| c as f32);
        s.mem_clock_mhz = dev.clock_info(Clock::Memory).ok().map(|c| c as f32);
        s.fan_rpm = dev.fan_speed_rpm(0).ok();

        let pdev = s.info.pdev.clone();
        let last = self.last_ts.get(&idx).copied();
        if let Ok(samples) = dev.process_utilization_stats(last) {
            let mut max_ts = last.unwrap_or(0);
            for p in samples {
                max_ts = max_ts.max(p.timestamp);
                let (util, engine) = [
                    (p.sm_util, "3D"),
                    (p.enc_util, "Video Encode"),
                    (p.dec_util, "Video Decode"),
                ]
                .into_iter()
                .max_by_key(|(u, _)| *u)
                .unwrap_or((0, "3D"));
                let e = per_proc.entry(p.pid as i32).or_default();
                if util as f32 >= e.util {
                    e.util = util as f32;
                    e.engine = Some(engine.to_string());
                }
                e.pdev = pdev.clone();
            }
            self.last_ts.insert(idx, max_ts);
        }
        let procs = dev
            .running_graphics_processes()
            .unwrap_or_default()
            .into_iter()
            .chain(dev.running_compute_processes().unwrap_or_default());
        for p in procs {
            if let UsedGpuMemory::Used(bytes) = p.used_gpu_memory {
                let e = per_proc.entry(p.pid as i32).or_default();
                e.vram = e.vram.max(bytes);
                e.pdev = pdev.clone();
            }
        }
    }
}
