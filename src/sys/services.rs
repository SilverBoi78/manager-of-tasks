//! systemd services (the Services tab).

use std::collections::BTreeSet;
use std::process::Command;

#[derive(Clone, Debug, Default)]
pub struct Service {
    pub name: String,
    pub description: String,
    pub pid: Option<u32>,
    /// "active", "inactive", "failed", ...
    pub active: String,
    /// "running", "dead", "exited", ...
    pub sub: String,
    /// "enabled", "disabled", "static", "masked", ...
    pub file_state: String,
}

impl Service {
    pub fn status(&self) -> &'static str {
        match (self.active.as_str(), self.sub.as_str()) {
            (_, "running") => "Running",
            ("active", "exited") => "Running (exited)",
            ("active", _) => "Running",
            ("activating", _) => "Starting",
            ("deactivating", _) => "Stopping",
            ("failed", _) => "Failed",
            _ => "Stopped",
        }
    }

    pub fn is_running(&self) -> bool {
        self.active == "active" || self.active == "activating" || self.active == "reloading"
    }

    pub fn display_name(&self) -> &str {
        self.name.strip_suffix(".service").unwrap_or(&self.name)
    }
}

fn systemctl(user: bool) -> Command {
    let mut c = Command::new("systemctl");
    if user {
        c.arg("--user");
    }
    c.args(["--no-pager", "--no-legend", "--plain"]);
    c
}

fn first_column(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| n.ends_with(".service") && !n.contains("@.service"))
        .map(str::to_string)
        .collect()
}

pub fn list(user: bool) -> Result<Vec<Service>, String> {
    let units = systemctl(user)
        .args(["list-units", "--type=service", "--all"])
        .output()
        .map_err(|e| format!("Could not run systemctl: {e}"))?;
    let files = systemctl(user)
        .args(["list-unit-files", "--type=service"])
        .output()
        .map_err(|e| format!("Could not run systemctl: {e}"))?;
    let mut names: BTreeSet<String> = first_column(&units.stdout).into_iter().collect();
    names.extend(first_column(&files.stdout));
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = systemctl(user);
    cmd.args([
        "show",
        "-p",
        "Id,Description,MainPID,ActiveState,SubState,UnitFileState,LoadState",
        "--",
    ]);
    cmd.args(&names);
    let out = cmd
        .output()
        .map_err(|e| format!("Could not run systemctl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut services = Vec::new();
    let mut seen = BTreeSet::new();
    for block in text.split("\n\n") {
        let mut s = Service::default();
        let mut load = String::new();
        for line in block.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k {
                "Id" => s.name = v.to_string(),
                "Description" => s.description = v.to_string(),
                "MainPID" => s.pid = v.parse().ok().filter(|&p| p != 0),
                "ActiveState" => s.active = v.to_string(),
                "SubState" => s.sub = v.to_string(),
                "UnitFileState" => s.file_state = v.to_string(),
                "LoadState" => load = v.to_string(),
                _ => {}
            }
        }
        // Aliases (e.g. dbus-org.freedesktop.Avahi.service) resolve to the same Id.
        if !s.name.is_empty() && load != "not-found" && seen.insert(s.name.clone()) {
            services.push(s);
        }
    }
    Ok(services)
}

/// Run `systemctl <verb> <unit>`. For system services, systemd asks the
/// desktop's polkit agent for authentication when needed.
pub fn action(user: bool, verb: &str, unit: &str) -> Result<(), String> {
    let mut c = Command::new("systemctl");
    if user {
        c.arg("--user");
    }
    let out = c
        .args([verb, "--", unit])
        .output()
        .map_err(|e| format!("Could not run systemctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("systemctl {verb} {unit} failed")
        } else {
            err
        })
    }
}
