//! Minimal freedesktop icon theme lookup for application icons.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct IconIndex {
    /// icon name -> (score, path); lower score is better.
    map: HashMap<String, (u32, PathBuf)>,
}

fn current_theme() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let kde = std::fs::read_to_string(format!("{home}/.config/kdeglobals")).ok();
    if let Some(text) = kde {
        let mut in_icons = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_icons = line == "[Icons]";
            } else if in_icons && let Some(v) = line.strip_prefix("Theme=") {
                return Some(v.trim().to_string());
            }
        }
    }
    let gtk = std::fs::read_to_string(format!("{home}/.config/gtk-3.0/settings.ini")).ok()?;
    gtk.lines()
        .find_map(|l| l.trim().strip_prefix("gtk-icon-theme-name="))
        .map(|s| s.trim().to_string())
}

fn icon_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![PathBuf::from(&home).join(".icons")];
    for d in super::apps::data_dirs() {
        dirs.push(d.join("icons"));
    }
    dirs
}

fn theme_inherits(name: &str, dirs: &[PathBuf]) -> Vec<String> {
    for d in dirs {
        if let Ok(text) = std::fs::read_to_string(d.join(name).join("index.theme"))
            && let Some(v) = text.lines().find_map(|l| l.strip_prefix("Inherits="))
        {
            return v.split(',').map(|s| s.trim().to_string()).collect();
        }
    }
    Vec::new()
}

impl IconIndex {
    pub fn build() -> Self {
        let dirs = icon_dirs();
        let mut themes: Vec<String> = Vec::new();
        let mut queue: Vec<String> = current_theme().into_iter().collect();
        while let Some(t) = queue.pop() {
            if themes.contains(&t) || themes.len() > 8 {
                continue;
            }
            for parent in theme_inherits(&t, &dirs).into_iter().rev() {
                queue.push(parent);
            }
            themes.push(t);
        }
        for fallback in ["breeze", "Adwaita", "hicolor"] {
            if !themes.iter().any(|t| t == fallback) {
                themes.push(fallback.into());
            }
        }
        let mut idx = IconIndex::default();
        for (rank, theme) in themes.iter().enumerate() {
            for d in &dirs {
                let root = d.join(theme);
                if root.is_dir() {
                    idx.scan(&root, rank as u32 * 1000, 0);
                }
            }
        }
        // Loose icons in pixmaps and the icons root come last.
        for d in super::apps::data_dirs() {
            idx.scan_flat(&d.join("pixmaps"), 90_000);
        }
        for d in &dirs {
            idx.scan_flat(d, 95_000);
        }
        idx
    }

    fn scan(&mut self, dir: &Path, base: u32, depth: u32) {
        if depth > 4 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() || (ft.is_symlink() && path.is_dir()) {
                self.scan(&path, base, depth + 1);
            } else {
                self.add(&path, base);
            }
        }
    }

    fn scan_flat(&mut self, dir: &Path, base: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            self.add(&e.path(), base);
        }
    }

    fn add(&mut self, path: &Path, base: u32) {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return;
        };
        if ext != "png" && ext != "svg" {
            return;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return;
        };
        let p = path.to_string_lossy();
        // Prefer scalable, then sizes near 32-64px, and "apps" context.
        let size_score = if ext == "svg" {
            0
        } else if p.contains("/48") || p.contains("x48") || p.contains("/64") || p.contains("x64") {
            1
        } else if p.contains("/32") || p.contains("x32") || p.contains("/128") || p.contains("x128")
        {
            2
        } else if p.contains("/256") || p.contains("x256") {
            3
        } else {
            5
        };
        let ctx_score = if p.contains("/apps") { 0 } else { 20 };
        let small_penalty = if p.contains("/16")
            || p.contains("/22")
            || p.contains("/24")
            || p.contains("symbolic")
        {
            40
        } else {
            0
        };
        let score = base + size_score + ctx_score + small_penalty;
        match self.map.get(stem) {
            Some((old, _)) if *old <= score => {}
            _ => {
                self.map
                    .insert(stem.to_string(), (score, path.to_path_buf()));
            }
        }
    }

    /// Resolve an icon name (or absolute path) to a file.
    pub fn lookup(&self, name: &str) -> Option<PathBuf> {
        if name.starts_with('/') {
            let p = PathBuf::from(name);
            return p.exists().then_some(p);
        }
        let name = name.trim_end_matches(".png").trim_end_matches(".svg");
        self.map.get(name).map(|(_, p)| p.clone())
    }
}
