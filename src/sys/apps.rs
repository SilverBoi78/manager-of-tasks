//! Desktop entries (.desktop files) and app detection from systemd cgroups.
//!
//! Modern desktops (KDE Plasma, GNOME) launch every application in its own
//! systemd unit named like `app-org.kde.konsole@<id>.service` or
//! `app-flatpak-org.mozilla.firefox-1234.scope`. We use that to group the
//! processes of an application together, the way Windows groups an app's
//! processes on the Processes tab.

use crate::util::systemd_unescape;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub id: String,
    pub name: Arc<str>,
    pub icon: Option<Arc<str>>,
    pub no_display: bool,
}

#[derive(Default)]
pub struct DesktopDb {
    entries: Vec<DesktopEntry>,
    by_id: HashMap<String, usize>,
    by_exec: HashMap<String, usize>,
    by_wmclass: HashMap<String, usize>,
}

pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".local/share")),
    ];
    let sys =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    for extra in [
        format!("{home}/.local/share/flatpak/exports/share"),
        "/var/lib/flatpak/exports/share".into(),
        "/var/lib/snapd/desktop".into(),
    ] {
        let p = PathBuf::from(extra);
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    dirs
}

/// Parse the `[Desktop Entry]` group of a .desktop file into key/value pairs.
pub fn parse_desktop_file(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.entry(k.trim().to_string())
                .or_insert_with(|| v.trim().to_string());
        }
    }
    out
}

/// The program name from an Exec= line, e.g. "/usr/bin/firefox %u" -> "firefox".
pub fn exec_program(exec: &str) -> Option<String> {
    let mut words = exec.split_whitespace().filter(|w| !w.contains('='));
    let mut prog = words.next()?;
    if prog == "env" {
        prog = words.next()?;
    }
    if prog.ends_with("flatpak") {
        // flatpak run [options] app.id
        return exec
            .split_whitespace()
            .rev()
            .find(|w| w.contains('.') && !w.starts_with('-') && !w.starts_with('@'))
            .map(|s| s.to_string());
    }
    let prog = prog.trim_matches('"');
    Some(Path::new(prog).file_name()?.to_string_lossy().to_string())
}

impl DesktopDb {
    pub fn load() -> Self {
        let mut db = DesktopDb::default();
        for dir in data_dirs() {
            let apps = dir.join("applications");
            db.scan(&apps, &apps);
        }
        db
    }

    fn scan(&mut self, root: &Path, dir: &Path) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                self.scan(root, &path);
                continue;
            }
            if path.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let id = path
                .strip_prefix(root)
                .map(|p| {
                    p.to_string_lossy()
                        .trim_end_matches(".desktop")
                        .replace('/', "-")
                })
                .unwrap_or_default();
            let key = id.to_lowercase();
            if self.by_id.contains_key(&key) {
                continue; // earlier data dirs take precedence
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let kv = parse_desktop_file(&text);
            if kv.get("Type").is_some_and(|t| t != "Application") {
                continue;
            }
            let Some(name) = kv.get("Name") else { continue };
            let exec = kv.get("Exec").cloned();
            let entry = DesktopEntry {
                id: id.clone(),
                name: name.as_str().into(),
                icon: kv
                    .get("Icon")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.as_str().into()),
                no_display: kv.get("NoDisplay").is_some_and(|v| v == "true")
                    || kv.get("Hidden").is_some_and(|v| v == "true"),
            };
            let idx = self.entries.len();
            self.entries.push(entry);
            self.by_id.insert(key, idx);
            if let Some(class) = kv.get("StartupWMClass") {
                self.by_wmclass.entry(class.to_lowercase()).or_insert(idx);
            }
            if let Some(prog) = exec.as_deref().and_then(exec_program) {
                let prog = prog.to_lowercase();
                // Prefer visible entries for an executable.
                match self.by_exec.get(&prog) {
                    Some(&old) if !self.entries[old].no_display => {}
                    _ => {
                        self.by_exec.insert(prog, idx);
                    }
                }
            }
        }
    }

    pub fn by_id(&self, id: &str) -> Option<&DesktopEntry> {
        let key = id.to_lowercase();
        let key = key.trim_end_matches(".desktop");
        if let Some(&i) = self.by_id.get(key) {
            return Some(&self.entries[i]);
        }
        if let Some(&i) = self.by_wmclass.get(key) {
            return Some(&self.entries[i]);
        }
        if let Some(&i) = self.by_exec.get(key) {
            return Some(&self.entries[i]);
        }
        // "org.kde.konsole" -> "konsole"
        let last = key.rsplit('.').next()?;
        if last != key
            && let Some(&i) = self.by_id.get(last).or_else(|| self.by_exec.get(last))
        {
            return Some(&self.entries[i]);
        }
        None
    }

    pub fn by_exec(&self, prog: &str) -> Option<&DesktopEntry> {
        self.by_exec
            .get(&prog.to_lowercase())
            .map(|&i| &self.entries[i])
    }
}

/// What kind of cgroup unit a process lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitKind {
    /// A launched application; contains the desktop file id.
    App(String),
    /// An app started by XDG autostart or D-Bus activation (tray apps, daemons).
    BackgroundApp(String),
    /// A plain systemd service.
    Service,
    /// Anything else (session scopes, init.scope, ...).
    Other,
}

/// Parse the systemd unit of a process from /proc/<pid>/cgroup contents.
/// Returns the unit name and its kind.
pub fn classify_cgroup(cgroup: &str) -> (String, UnitKind) {
    // cgroup v2: "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-foo@x.service"
    let path = cgroup
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .or_else(|| {
            cgroup
                .lines()
                .find_map(|l| l.split(":name=systemd:").nth(1))
        })
        .unwrap_or("");
    // Skip trailing sub-cgroups that aren't units (e.g. ".../app.service/tab-1").
    let unit = path
        .rsplit('/')
        .find(|c| c.ends_with(".service") || c.ends_with(".scope"))
        .unwrap_or("")
        .to_string();
    if unit.is_empty() {
        return (unit, UnitKind::Other);
    }
    if let Some(id) = parse_app_unit(&unit) {
        let background = unit.contains("@autostart")
            || id.starts_with("dbus-")
            || id.starts_with("dbus:")
            || unit.starts_with("app-dbus");
        return if background {
            (unit, UnitKind::BackgroundApp(id))
        } else {
            (unit, UnitKind::App(id))
        };
    }
    if let Some(rest) = unit.strip_prefix("snap.")
        && let Some(name) = rest.split('.').next()
    {
        return (unit.clone(), UnitKind::App(name.to_string()));
    }
    if unit.ends_with(".service") && !unit.starts_with("user@") {
        (unit, UnitKind::Service)
    } else {
        (unit, UnitKind::Other)
    }
}

/// "app-org.kde.konsole@abc.service" -> "org.kde.konsole"
/// "app-flatpak-org.mozilla.firefox-1234.scope" -> "org.mozilla.firefox"
/// "app-gnome-google\x2dchrome-1234.scope" -> "google-chrome"
pub fn parse_app_unit(unit: &str) -> Option<String> {
    let s = unit
        .strip_suffix(".service")
        .or_else(|| unit.strip_suffix(".scope"))?;
    let s = s.strip_prefix("app-")?;
    let s = s.split('@').next()?;
    let mut parts: Vec<&str> = s.split('-').collect();
    if parts.len() > 1
        && matches!(
            parts[0],
            "flatpak"
                | "gnome"
                | "kde"
                | "snap"
                | "cinnamon"
                | "xfce"
                | "niri"
                | "hyprland"
                | "sway"
        )
    {
        parts.remove(0);
    }
    if parts.len() > 1 {
        let last = parts[parts.len() - 1];
        let looks_random = last.chars().all(|c| c.is_ascii_hexdigit()) && !last.is_empty();
        if looks_random {
            parts.pop();
        }
    }
    let id = systemd_unescape(&parts.join("-"));
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_units() {
        assert_eq!(
            parse_app_unit("app-org.kde.konsole@abc123.service").unwrap(),
            "org.kde.konsole"
        );
        assert_eq!(
            parse_app_unit("app-Alacritty@c9c1a8e7614c41a082fa312a5d2ad173.service").unwrap(),
            "Alacritty"
        );
        assert_eq!(
            parse_app_unit("app-flatpak-org.mozilla.firefox-1234.scope").unwrap(),
            "org.mozilla.firefox"
        );
        assert_eq!(
            parse_app_unit(r"app-gnome-google\x2dchrome-5678.scope").unwrap(),
            "google-chrome"
        );
        assert_eq!(parse_app_unit("plasma-kwin_wayland.service"), None);
    }

    #[test]
    fn classify() {
        let (_, k) = classify_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.kde.kdeconnect.daemon@autostart.service\n",
        );
        assert_eq!(
            k,
            UnitKind::BackgroundApp("org.kde.kdeconnect.daemon".into())
        );
        let (u, k) = classify_cgroup("0::/system.slice/NetworkManager.service\n");
        assert_eq!(
            (u.as_str(), k),
            ("NetworkManager.service", UnitKind::Service)
        );
        let (_, k) = classify_cgroup("0::/user.slice/user-1000.slice/session-3.scope\n");
        assert_eq!(k, UnitKind::Other);
    }

    #[test]
    fn exec() {
        assert_eq!(
            exec_program("/usr/lib/firefox/firefox %u").unwrap(),
            "firefox"
        );
        assert_eq!(exec_program("env FOO=1 code --new-window").unwrap(), "code");
        assert_eq!(exec_program("/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=firefox org.mozilla.firefox @@u %u @@").unwrap(), "org.mozilla.firefox");
    }
}
