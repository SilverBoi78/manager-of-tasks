//! Per-process GPU usage from DRM fdinfo (`/proc/<pid>/fdinfo/<fd>`).
//!
//! Every open DRM file descriptor exposes `drm-engine-<name>` busy time in
//! nanoseconds (amdgpu, i915, msm, panfrost, v3d, ...) or `drm-cycles-<name>`
//! with `drm-total-cycles-<name>` (xe). We diff these between samples to get
//! utilization per process and per engine, the same way nvtop does.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct ProcGpu {
    /// Highest engine utilization of the process, 0..100.
    pub util: f32,
    /// Engine with the highest utilization, e.g. "3D".
    pub engine: Option<String>,
    pub pdev: String,
    pub vram: u64,
    pub shared: u64,
}

#[derive(Default)]
struct ClientPrev {
    engines: HashMap<String, u64>,
    cycles: HashMap<String, (u64, u64)>,
    at: Option<Instant>,
}

/// Utilization per (pci device, engine name), 0..100.
pub type EngineUtil = HashMap<String, HashMap<String, f32>>;

pub struct FdInfoScanner {
    /// DRM fds of each pid, with the tick they were last scanned.
    drm_fds: HashMap<i32, (Vec<String>, u64)>,
    prev: HashMap<(String, u64), ClientPrev>,
    tick: u64,
}

#[derive(Default)]
struct Client {
    pdev: String,
    id: u64,
    engines: HashMap<String, u64>,
    cycles: HashMap<String, u64>,
    total_cycles: HashMap<String, u64>,
    vram: u64,
    shared: u64,
}

impl FdInfoScanner {
    pub fn new() -> Self {
        Self {
            drm_fds: HashMap::new(),
            prev: HashMap::new(),
            tick: 0,
        }
    }

    pub fn scan(&mut self, pids: &[i32]) -> (HashMap<i32, ProcGpu>, EngineUtil) {
        self.tick += 1;
        let now = Instant::now();
        let alive: HashSet<i32> = pids.iter().copied().collect();
        self.drm_fds.retain(|pid, _| alive.contains(pid));

        let mut seen: HashSet<(String, u64)> = HashSet::new();
        let mut per_proc: HashMap<i32, ProcGpu> = HashMap::new();
        let mut engine_util: EngineUtil = HashMap::new();
        let mut next_prev: HashMap<(String, u64), ClientPrev> = HashMap::new();

        for &pid in pids {
            let rescan = match self.drm_fds.get(&pid) {
                None => true,
                Some((_, at)) => self.tick - at >= 5,
            };
            if rescan {
                self.drm_fds.insert(pid, (find_drm_fds(pid), self.tick));
            }
            let Some((fds, _)) = self.drm_fds.get(&pid) else {
                continue;
            };
            if fds.is_empty() {
                continue;
            }
            let mut proc_engines: HashMap<String, f32> = HashMap::new();
            let mut proc = ProcGpu::default();
            for fd in fds {
                let Some(c) = read_client(pid, fd) else {
                    continue;
                };
                let key = (c.pdev.clone(), c.id);
                if !seen.insert(key.clone()) {
                    continue;
                }
                let prev = self.prev.remove(&key);
                let mut cur = ClientPrev {
                    engines: c.engines.clone(),
                    cycles: HashMap::new(),
                    at: Some(now),
                };
                for (name, ns) in &c.engines {
                    if let Some(p) = &prev
                        && let (Some(old), Some(at)) = (p.engines.get(name), p.at)
                    {
                        let wall = (now - at).as_nanos() as f64;
                        if wall > 0.0 {
                            let u = (ns.saturating_sub(*old) as f64 / wall * 100.0) as f32;
                            *proc_engines.entry(name.clone()).or_default() += u;
                        }
                    }
                }
                for (name, cyc) in &c.cycles {
                    let Some(total) = c.total_cycles.get(name) else {
                        continue;
                    };
                    cur.cycles.insert(name.clone(), (*cyc, *total));
                    if let Some(p) = &prev
                        && let Some((oc, ot)) = p.cycles.get(name)
                    {
                        let dt = total.saturating_sub(*ot);
                        if dt > 0 {
                            let u = (cyc.saturating_sub(*oc) as f64 / dt as f64 * 100.0) as f32;
                            *proc_engines.entry(name.clone()).or_default() += u;
                        }
                    }
                }
                proc.vram += c.vram;
                proc.shared += c.shared;
                proc.pdev = c.pdev.clone();
                next_prev.insert(key, cur);
            }
            if proc.pdev.is_empty() {
                continue;
            }
            let dev = engine_util.entry(proc.pdev.clone()).or_default();
            for (name, u) in &proc_engines {
                let u = u.min(100.0);
                *dev.entry(name.clone()).or_default() += u;
                if u > proc.util {
                    proc.util = u;
                    proc.engine = Some(engine_label(name));
                }
            }
            per_proc.insert(pid, proc);
        }
        for dev in engine_util.values_mut() {
            for v in dev.values_mut() {
                *v = v.min(100.0);
            }
        }
        self.prev = next_prev;
        (per_proc, engine_util)
    }
}

fn find_drm_fds(pid: i32) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return out;
    };
    for e in dir.flatten() {
        if let Ok(target) = std::fs::read_link(e.path())
            && target
                .as_os_str()
                .as_encoded_bytes()
                .starts_with(b"/dev/dri/")
        {
            out.push(e.file_name().to_string_lossy().to_string());
        }
    }
    out
}

fn kib(v: &str) -> u64 {
    let mut it = v.split_whitespace();
    let n: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    match it.next() {
        Some("KiB") => n * 1024,
        Some("MiB") => n * 1024 * 1024,
        Some("GiB") => n * 1024 * 1024 * 1024,
        _ => n,
    }
}

fn read_client(pid: i32, fd: &str) -> Option<Client> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{fd}")).ok()?;
    let mut c = Client::default();
    let mut has_id = false;
    let mut vram_resident = None;
    let mut vram_memory = None;
    let mut vram_total = None;
    let mut sys_resident = None;
    let mut gtt = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k {
            "drm-pdev" => c.pdev = v.to_string(),
            "drm-client-id" => {
                c.id = v.parse().ok()?;
                has_id = true;
            }
            "drm-resident-vram" | "drm-resident-vram0" => {
                vram_resident = Some(vram_resident.unwrap_or(0) + kib(v))
            }
            "drm-memory-vram" => vram_memory = Some(kib(v)),
            "drm-total-vram" | "drm-total-vram0" => vram_total = Some(kib(v)),
            "drm-resident-gtt" | "drm-memory-gtt" => gtt = Some(gtt.unwrap_or(0).max(kib(v))),
            "drm-resident-system" | "drm-resident-system0" | "drm-resident-memory" => {
                sys_resident = Some(kib(v))
            }
            _ => {
                if let Some(name) = k.strip_prefix("drm-engine-") {
                    if !name.starts_with("capacity") {
                        c.engines.insert(name.to_string(), kib(v));
                    }
                } else if let Some(name) = k.strip_prefix("drm-total-cycles-") {
                    c.total_cycles.insert(name.to_string(), kib(v));
                } else if let Some(name) = k.strip_prefix("drm-cycles-") {
                    c.cycles.insert(name.to_string(), kib(v));
                }
            }
        }
    }
    if !has_id {
        return None;
    }
    c.vram = vram_resident.or(vram_memory).or(vram_total).unwrap_or(0);
    c.shared = gtt.or(sys_resident).unwrap_or(0);
    Some(c)
}

/// Windows-style engine names.
pub fn engine_label(name: &str) -> String {
    match name {
        "gfx" | "render" | "rcs" | "3d" => "3D".into(),
        "compute" | "ccs" => "Compute".into(),
        "dma" | "copy" | "bcs" | "sdma" => "Copy".into(),
        "dec" | "video" | "vcs" | "nvdec" => "Video Decode".into(),
        "enc" | "nvenc" => "Video Encode".into(),
        "video-enhance" | "vecs" => "Video Processing".into(),
        "vcn" | "vcn_unified" | "media" => "Video Codec".into(),
        "jpeg" | "nvjpg" => "JPEG".into(),
        other => {
            let mut s = other.replace(['-', '_'], " ");
            if let Some(f) = s.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            s
        }
    }
}
