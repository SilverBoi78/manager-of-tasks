pub mod app_history;
pub mod details;
pub mod dialogs;
pub mod performance;
pub mod processes;
pub mod services;
pub mod settings;
pub mod startup;
pub mod users;
pub mod widgets;

use crate::actions::Actions;
use crate::settings::Settings;
use crate::sys::Snapshot;
use crate::sys::icons::IconIndex;
use crate::theme::Palette;

/// Requests to jump to another page with something selected.
pub enum Nav {
    Details(i32),
    Processes(i32),
    Services(String),
}

/// Everything a page needs to draw itself.
pub struct PageCtx<'a> {
    pub snap: &'a Snapshot,
    pub settings: &'a mut Settings,
    pub actions: &'a Actions,
    pub icons: Option<&'a IconIndex>,
    pub search: &'a str,
    pub dialogs: &'a mut dialogs::Dialogs,
    pub pal: Palette,
    pub nav: &'a mut Option<Nav>,
}

impl PageCtx<'_> {
    pub fn matches(&self, text: &str) -> bool {
        self.search.is_empty() || text.to_lowercase().contains(&self.search.to_lowercase())
    }
}

/// Processes considered critical to the session; ending them asks for confirmation.
pub fn is_critical(name: &str) -> bool {
    matches!(
        name,
        "systemd"
            | "init"
            | "kwin_wayland"
            | "kwin_x11"
            | "plasmashell"
            | "gnome-shell"
            | "Xwayland"
            | "Xorg"
            | "dbus-daemon"
            | "dbus-broker"
            | "pipewire"
            | "sddm"
            | "gdm"
            | "startplasma-wayland"
            | "startplasma-x11"
    )
}

/// Loads data on a background thread and keeps the latest result.
pub struct Loader<T> {
    data: std::sync::Arc<std::sync::Mutex<Option<T>>>,
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stale: bool,
    last: Option<std::time::Instant>,
}

impl<T> Default for Loader<T> {
    fn default() -> Self {
        Self {
            data: Default::default(),
            busy: Default::default(),
            stale: true,
            last: None,
        }
    }
}

impl<T: Send + 'static> Loader<T> {
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    /// Start a reload when stale (or older than `every`), then return the current data.
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        every: Option<std::time::Duration>,
        load: impl FnOnce() -> T + Send + 'static,
    ) -> std::sync::MutexGuard<'_, Option<T>> {
        use std::sync::atomic::Ordering;
        let expired = match (every, self.last) {
            (Some(e), Some(l)) => l.elapsed() >= e,
            _ => false,
        };
        if (self.stale || expired) && !self.busy.load(Ordering::Acquire) {
            self.stale = false;
            self.last = Some(std::time::Instant::now());
            self.busy.store(true, Ordering::Release);
            let data = self.data.clone();
            let busy = self.busy.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let v = load();
                if let Ok(mut d) = data.lock() {
                    *d = Some(v);
                }
                busy.store(false, Ordering::Release);
                ctx.request_repaint();
            });
        }
        if let Some(e) = every {
            ctx.request_repaint_after(e);
        }
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_loading(&self) -> bool {
        self.busy.load(std::sync::atomic::Ordering::Acquire)
    }
}
