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
            // Shortcuts that open a URL with a launcher (e.g. a Steam game's
            // "steam steam://rungameid/123") don't describe the launcher itself.
            if let Some(prog) = exec
                .as_deref()
                .filter(|e| !e.contains("://"))
                .and_then(exec_program)
            {
                let prog = prog.to_lowercase();
                let is_named_after = |id: &str| {
                    let id = id.to_lowercase();
                    id == prog || id.rsplit('.').next() == Some(prog.as_str())
                };
                let new = &self.entries[idx];
                let replace = match self.by_exec.get(&prog) {
                    None => true,
                    Some(&old) => {
                        let old = &self.entries[old];
                        // Prefer the entry named after the program, then visible entries.
                        let old_rank = (is_named_after(&old.id), !old.no_display);
                        let new_rank = (is_named_after(&new.id), !new.no_display);
                        new_rank > old_rank
                    }
                };
                if replace {
                    self.by_exec.insert(prog, idx);
                }
            }
        }
    }

    pub fn by_id(&self, id: &str) -> Option<&DesktopEntry> {
        // A program path (app started from a file manager): match its file name.
        if id.contains('/') {
            return self.by_exec(id.rsplit('/').next().unwrap_or(id));
        }
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
    let unit = unit_from_path(path);
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

/// The systemd unit a cgroup path belongs to.
///
/// The unit starts after the last slice (or the `user@UID.service` manager) and
/// ends at the first `.service`/`.scope` component, so sub-cgroups after the
/// unit (e.g. `.../app.service/tab-1`) are ignored.
fn unit_from_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let is_unit = |c: &str| c.ends_with(".service") || c.ends_with(".scope");
    let is_container =
        |c: &str| c.ends_with(".slice") || (c.starts_with("user@") && c.ends_with(".service"));
    let start = parts
        .iter()
        .rposition(|c| is_container(c))
        .map_or(0, |i| i + 1);
    match parts[start..].iter().position(|c| is_unit(c)) {
        Some(end) => parts[start..=start + end].join("/"),
        None => String::new(),
    }
}

/// Friendly name for an app id that is a program path ("/home/me/Game/game.x86_64" -> "game").
pub fn app_display_name(id: &str) -> String {
    let base = id.rsplit('/').next().unwrap_or(id);
    let mut name = base;
    for suffix in [
        ".x86_64",
        ".x86",
        ".AppImage",
        ".appimage",
        ".sh",
        ".bin",
        ".exe",
    ] {
        if let Some(n) = name.strip_suffix(suffix) {
            name = n;
            break;
        }
    }
    if name.is_empty() {
        base.to_string()
    } else {
        name.to_string()
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
        // Programs started from a file manager are named after their (escaped) path.
        let (u, k) = classify_cgroup(
            r"0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-\x2fhome\x2fme\x2fDownloads\x2fGame\x2dlinux\x2fGame.x86_64@ee0a64.service",
        );
        assert_eq!(
            u,
            r"app-\x2fhome\x2fme\x2fDownloads\x2fGame\x2dlinux\x2fGame.x86_64@ee0a64.service"
        );
        assert_eq!(
            k,
            UnitKind::App("/home/me/Downloads/Game-linux/Game.x86_64".into())
        );
        assert_eq!(
            app_display_name("/home/me/Downloads/Game-linux/Game.x86_64"),
            "Game"
        );
        // Sub-cgroups below a unit and the user manager's own scope.
        let (u, _) = classify_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.kde.konsole@x.service/tab-1\n",
        );
        assert_eq!(u, "app-org.kde.konsole@x.service");
        let (u, _) =
            classify_cgroup("0::/user.slice/user-1000.slice/user@1000.service/init.scope\n");
        assert_eq!(u, "init.scope");
    }

    #[test]
    fn url_shortcuts_do_not_claim_launcher() {
        let dir = std::env::temp_dir().join(format!("mot-apps-{}", std::process::id()));
        let apps = dir.join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("Overwatch.desktop"), "[Desktop Entry]\nType=Application\nName=Overwatch\nExec=steam steam://rungameid/2357570\n").unwrap();
        std::fs::write(
            apps.join("steam.desktop"),
            "[Desktop Entry]\nType=Application\nName=Steam\nExec=/usr/bin/steam %U\n",
        )
        .unwrap();
        let mut db = DesktopDb::default();
        db.scan(&apps, &apps);
        assert_eq!(db.by_exec("steam").map(|e| e.name.as_ref()), Some("Steam"));
        std::fs::remove_dir_all(&dir).unwrap();
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
