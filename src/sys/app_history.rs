//! Accumulated resource usage per app (the App history tab), persisted to disk.

use super::process::{Class, Proc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub name: String,
    pub icon: Option<String>,
    pub cpu_secs: f64,
    pub disk_bytes: u64,
    pub gpu_secs: f64,
    /// Unix time of the last activity.
    pub last_seen: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppHistory {
    /// Unix time when tracking started.
    pub since: u64,
    pub apps: HashMap<String, HistoryEntry>,
    #[serde(skip)]
    pub dirty: bool,
}

fn path() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("manager-of-tasks/app_history.json")
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl AppHistory {
    pub fn load() -> Self {
        let mut h: AppHistory = std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if h.since == 0 {
            h.since = now_unix();
        }
        h
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
        self.dirty = false;
    }

    pub fn clear(&mut self) {
        self.apps.clear();
        self.since = now_unix();
        self.dirty = true;
        self.save();
    }

    /// Add usage since the last sample. `deltas` holds (cpu seconds, disk bytes) per pid.
    pub fn accumulate(
        &mut self,
        procs: &[Proc],
        deltas: &HashMap<i32, (f64, u64)>,
        dt: f64,
        all: bool,
    ) {
        let now = now_unix();
        for p in procs {
            let (key, name) = match (&p.app_id, &p.app_name) {
                (Some(id), Some(name)) if p.class == Class::App => {
                    (id.to_string(), name.to_string())
                }
                _ if all && p.class != Class::Kernel => {
                    (format!("proc:{}", p.exe_name), p.name.to_string())
                }
                _ => continue,
            };
            let Some(&(cpu, disk)) = deltas.get(&p.pid) else {
                continue;
            };
            let gpu = p.gpu as f64 / 100.0 * dt;
            if cpu == 0.0 && disk == 0 && gpu == 0.0 {
                continue;
            }
            let e = self.apps.entry(key).or_insert_with(|| HistoryEntry {
                name,
                icon: p.icon.as_deref().map(str::to_string),
                ..Default::default()
            });
            e.cpu_secs += cpu;
            e.disk_bytes += disk;
            e.gpu_secs += gpu;
            e.last_seen = now;
            if e.icon.is_none() {
                e.icon = p.icon.as_deref().map(str::to_string);
            }
            self.dirty = true;
        }
    }
}
