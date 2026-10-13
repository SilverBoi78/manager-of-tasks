//! XDG autostart entries (the Startup apps tab).

use super::apps::{exec_program, parse_desktop_file};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct StartupItem {
    /// File name, e.g. "org.kde.kdeconnect.daemon.desktop".
    pub file: String,
    pub name: String,
    pub command: String,
    pub icon: Option<String>,
    pub comment: Option<String>,
    pub enabled: bool,
    /// True when the entry comes from a system directory (possibly overridden by the user).
    pub system: bool,
    /// Entry is limited to other desktops via OnlyShowIn / NotShowIn.
    pub other_desktop: bool,
    pub path: PathBuf,
}

impl StartupItem {
    pub fn program(&self) -> String {
        exec_program(&self.command).unwrap_or_else(|| self.command.clone())
    }
}

fn user_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        })
        .join("autostart")
}

fn system_dirs() -> Vec<PathBuf> {
    std::env::var("XDG_CONFIG_DIRS")
        .unwrap_or_else(|_| "/etc/xdg".into())
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(s).join("autostart"))
        .collect()
}

pub fn list() -> Vec<StartupItem> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .map(|s| s.to_lowercase())
        .collect();
    let mut items: Vec<StartupItem> = Vec::new();
    let mut dirs = vec![(user_dir(), false)];
    dirs.extend(system_dirs().into_iter().map(|d| (d, true)));
    for (dir, system) in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let file = e.file_name().to_string_lossy().to_string();
            if let Some(existing) = items.iter_mut().find(|i| i.file == file) {
                // A user file overrides the system one with the same name.
                existing.system |= system;
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let kv = parse_desktop_file(&text);
            let hidden = kv.get("Hidden").is_some_and(|v| v == "true");
            let gnome_off = kv
                .get("X-GNOME-Autostart-enabled")
                .is_some_and(|v| v == "false");
            let in_list = |key: &str| {
                kv.get(key).map(|v| {
                    v.split(';')
                        .filter(|s| !s.is_empty())
                        .any(|s| desktops.contains(&s.to_lowercase()))
                })
            };
            let other_desktop =
                in_list("OnlyShowIn") == Some(false) || in_list("NotShowIn") == Some(true);
            items.push(StartupItem {
                name: kv
                    .get("Name")
                    .cloned()
                    .unwrap_or_else(|| file.trim_end_matches(".desktop").to_string()),
                command: kv.get("Exec").cloned().unwrap_or_default(),
                icon: kv.get("Icon").cloned(),
                comment: kv.get("Comment").cloned(),
                enabled: !hidden && !gnome_off,
                system,
                other_desktop,
                path,
                file,
            });
        }
    }
    items.sort_by_key(|i| i.name.to_lowercase());
    items
}

/// Enable or disable an autostart entry by writing a user override with `Hidden=`.
pub fn set_enabled(item: &StartupItem, enabled: bool) -> Result<(), String> {
    let dir = user_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(&item.file);
    let source = if target.exists() {
        target.clone()
    } else {
        item.path.clone()
    };
    let text = std::fs::read_to_string(&source).map_err(|e| e.to_string())?;
    let mut out = String::new();
    let mut in_main = false;
    let mut wrote = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_main && !wrote {
                out.push_str(&format!("Hidden={enabled_neg}\n", enabled_neg = !enabled));
                wrote = true;
            }
            in_main = t == "[Desktop Entry]";
        } else if in_main
            && (t.starts_with("Hidden=") || t.starts_with("X-GNOME-Autostart-enabled="))
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !wrote {
        out.push_str(&format!("Hidden={}\n", !enabled));
    }
    std::fs::write(&target, out).map_err(|e| e.to_string())
}

/// "Last BIOS time" from systemd-analyze, in seconds.
pub fn firmware_time() -> Option<f64> {
    let out = std::process::Command::new("systemd-analyze")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?;
    let part = line.split(['+', '=']).find(|p| p.contains("(firmware)"))?;
    parse_duration(part.trim().trim_end_matches("(firmware)").trim())
}

fn parse_duration(s: &str) -> Option<f64> {
    // e.g. "1min 2.345s", "12.3s", "850ms"
    let mut total = 0.0;
    for tok in s.split_whitespace() {
        if let Some(v) = tok.strip_suffix("ms") {
            total += v.parse::<f64>().ok()? / 1000.0;
        } else if let Some(v) = tok.strip_suffix("min") {
            total += v.parse::<f64>().ok()? * 60.0;
        } else {
            let v = tok.strip_suffix('s')?;
            total += v.parse::<f64>().ok()?;
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations() {
        assert_eq!(super::parse_duration("12.5s"), Some(12.5));
        assert_eq!(super::parse_duration("1min 2.5s"), Some(62.5));
        assert_eq!(super::parse_duration("850ms"), Some(0.85));
    }
}
