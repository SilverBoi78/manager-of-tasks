//! A short description of this system for bug reports (`--system-info`).

use crate::util::{read, read_trim};

pub fn system_info() -> String {
    let os = read("/etc/os-release")
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "Unknown".into());
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "unknown".into());
    let cpu = super::cpu::CpuSampler::new().sample();
    let mem = super::memory::MemSampler::new().sample();
    let (gpus, _) = super::gpu::GpuSampler::new().sample(&[]);

    let mut out = format!(
        "Manager of Tasks {}\nOS: {os}\nKernel: {}\nDesktop: {} ({})\nsystemd: {}\nCPU: {} ({} cores, {} threads)\nMemory: {:.1} GB\n",
        env!("CARGO_PKG_VERSION"),
        read_trim("/proc/sys/kernel/osrelease").unwrap_or_default(),
        env("XDG_CURRENT_DESKTOP"),
        env("XDG_SESSION_TYPE"),
        if std::path::Path::new("/run/systemd/system").exists() {
            "yes"
        } else {
            "no"
        },
        cpu.info.model,
        cpu.info.cores,
        cpu.info.logical,
        mem.total as f64 / 1073741824.0,
    );
    if gpus.is_empty() {
        out.push_str("GPU: none detected\n");
    }
    for g in &gpus {
        out.push_str(&format!(
            "GPU {}: {} [driver {} {}, {}]\n",
            g.info.index, g.info.name, g.info.driver, g.info.driver_version, g.info.pdev
        ));
    }
    out
}
