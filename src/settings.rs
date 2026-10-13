//! User settings persisted to ~/.config/manager-of-tasks/settings.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Page {
    #[default]
    Processes,
    Performance,
    AppHistory,
    Startup,
    Users,
    Details,
    Services,
    Settings,
}

impl Page {
    pub const MAIN: [Page; 7] = [
        Page::Processes,
        Page::Performance,
        Page::AppHistory,
        Page::Startup,
        Page::Users,
        Page::Details,
        Page::Services,
    ];

    pub fn from_arg(s: &str) -> Option<Page> {
        Some(
            match s.to_lowercase().replace(['-', '_', ' '], "").as_str() {
                "processes" => Page::Processes,
                "performance" => Page::Performance,
                "apphistory" | "history" => Page::AppHistory,
                "startup" | "startupapps" => Page::Startup,
                "users" => Page::Users,
                "details" => Page::Details,
                "services" => Page::Services,
                "settings" => Page::Settings,
                _ => return None,
            },
        )
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Processes => "Processes",
            Page::Performance => "Performance",
            Page::AppHistory => "App history",
            Page::Startup => "Startup apps",
            Page::Users => "Users",
            Page::Details => "Details",
            Page::Services => "Services",
            Page::Settings => "Settings",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Speed {
    High,
    #[default]
    Normal,
    Low,
    Paused,
}

impl Speed {
    pub const ALL: [Speed; 4] = [Speed::High, Speed::Normal, Speed::Low, Speed::Paused];

    pub fn interval(self) -> Option<Duration> {
        match self {
            Speed::High => Some(Duration::from_millis(500)),
            Speed::Normal => Some(Duration::from_secs(1)),
            Speed::Low => Some(Duration::from_secs(4)),
            Speed::Paused => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Speed::High => "High",
            Speed::Normal => "Normal",
            Speed::Low => "Low",
            Speed::Paused => "Paused",
        }
    }

    /// Length of the graph history in seconds.
    pub fn graph_seconds(self) -> u64 {
        let ms = self
            .interval()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1000);
        ms * crate::sys::HISTORY as u64 / 1000
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CpuGraph {
    #[default]
    Overall,
    Logical,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePref,
    pub start_page: Page,
    pub speed: Speed,
    pub always_on_top: bool,
    pub nav_expanded: bool,
    pub group_by_type: bool,
    pub heatmap: bool,
    pub show_full_account_name: bool,
    pub show_kernel_threads: bool,
    pub history_all_processes: bool,
    pub cpu_graph: CpuGraph,
    pub show_kernel_times: bool,
    pub confirm_efficiency: bool,
    pub process_columns: Vec<String>,
    pub details_columns: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            start_page: Page::Processes,
            speed: Speed::Normal,
            always_on_top: false,
            nav_expanded: false,
            group_by_type: true,
            heatmap: true,
            show_full_account_name: false,
            show_kernel_threads: false,
            history_all_processes: false,
            cpu_graph: CpuGraph::Overall,
            show_kernel_times: false,
            confirm_efficiency: true,
            process_columns: Vec::new(),
            details_columns: Vec::new(),
        }
    }
}

fn path() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        })
        .join("manager-of-tasks/settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}
