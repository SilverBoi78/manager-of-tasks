//! Things the user can do to processes: end, suspend, change priority, ...
//!
//! Operations that need more privileges than we have fall back to `pkexec`,
//! which shows the desktop's polkit password prompt (the Linux equivalent of
//! "Run as administrator").

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

pub enum Notice {
    Info(String),
    Error(String),
    /// Something changed; refresh data now.
    Refresh,
}

#[derive(Clone)]
pub struct Actions {
    tx: Sender<Notice>,
    ctx: egui::Context,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn threads_of(pid: i32) -> Vec<i32> {
    let mut t: Vec<i32> = std::fs::read_dir(format!("/proc/{pid}/task"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    if t.is_empty() {
        t.push(pid);
    }
    t
}

fn run_pkexec(args: &[String]) -> Result<(), String> {
    let out = Command::new("pkexec")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not run pkexec: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(()),
        Some(126) => Err("Authentication was cancelled.".into()),
        Some(127) => Err("Not authorized.".into()),
        _ => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    }
}

/// How long End task waits for processes to exit before killing them.
const END_TASK_GRACE: Duration = Duration::from_secs(3);

fn pidfd_open(pid: i32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open takes plain integers and returns a new fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    // SAFETY: on success the fd is new and owned by nobody else.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

/// Wait up to `grace` for the processes to exit, then SIGKILL the rest.
fn kill_survivors(mut fds: Vec<OwnedFd>, grace: Duration) {
    let deadline = Instant::now() + grace;
    loop {
        // A pidfd becomes readable once its process has exited.
        let mut polls: Vec<libc::pollfd> = fds
            .iter()
            .map(|fd| libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let left = deadline.saturating_duration_since(Instant::now());
        // SAFETY: `polls` is a valid array of `polls.len()` pollfds.
        unsafe {
            libc::poll(
                polls.as_mut_ptr(),
                polls.len() as libc::nfds_t,
                left.as_millis().min(100) as i32,
            )
        };
        let mut running = polls.iter().map(|p| p.revents == 0);
        fds.retain(|_| running.next().unwrap_or(false));
        if fds.is_empty() || left.is_zero() {
            break;
        }
    }
    for fd in fds {
        // SAFETY: pidfd_send_signal takes a valid pidfd and a null siginfo.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
    }
}

const IOPRIO_WHO_PROCESS: libc::c_long = 1;
const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
const IOPRIO_CLASS_IDLE: libc::c_long = 3;

pub fn signal_name(sig: i32) -> &'static str {
    match sig {
        libc::SIGKILL => "KILL",
        libc::SIGSTOP => "STOP",
        libc::SIGCONT => "CONT",
        _ => "TERM",
    }
}

impl Actions {
    pub fn new(tx: Sender<Notice>, ctx: egui::Context) -> Self {
        Self { tx, ctx }
    }

    pub fn notify(&self, n: Notice) {
        let _ = self.tx.send(n);
        self.ctx.request_repaint();
    }

    fn background(&self, f: impl FnOnce() -> Result<Option<String>, String> + Send + 'static) {
        let me = self.clone();
        std::thread::spawn(move || {
            match f() {
                Ok(Some(msg)) => me.notify(Notice::Info(msg)),
                Ok(None) => {}
                Err(e) => me.notify(Notice::Error(e)),
            }
            me.notify(Notice::Refresh);
        });
    }

    /// Send a signal, retrying with pkexec for processes we may not signal.
    ///
    /// SIGTERM is "End task": processes that are still running after a grace
    /// period (e.g. apps that only hide to the system tray) are killed, like
    /// End task on Windows always ends the app.
    pub fn signal(&self, pids: Vec<i32>, sig: i32) {
        self.background(move || {
            let mut denied = Vec::new();
            let mut ended = Vec::new();
            for &pid in &pids {
                // Opened before signalling, so the pid cannot be reused under us.
                let fd = (sig == libc::SIGTERM).then(|| pidfd_open(pid)).flatten();
                // SAFETY: kill(2) has no memory safety preconditions.
                if unsafe { libc::kill(pid, sig) } != 0 {
                    if errno() == libc::EPERM {
                        denied.push(pid);
                    }
                } else if let Some(fd) = fd {
                    ended.push(fd);
                }
            }
            if !ended.is_empty() {
                kill_survivors(ended, END_TASK_GRACE);
            }
            if denied.is_empty() {
                return Ok(None);
            }
            let mut args = vec!["kill".to_string(), format!("-{}", signal_name(sig))];
            args.extend(denied.iter().map(|p| p.to_string()));
            run_pkexec(&args).map(|_| None)
        });
    }

    pub fn set_nice(&self, pids: Vec<i32>, nice: i32) {
        self.background(move || {
            let mut denied = Vec::new();
            for &pid in &pids {
                for tid in threads_of(pid) {
                    // SAFETY: setpriority has no memory safety preconditions.
                    let rc =
                        unsafe { libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, nice) };
                    if rc != 0 && matches!(errno(), libc::EPERM | libc::EACCES) {
                        denied.push(tid);
                    }
                }
            }
            if denied.is_empty() {
                return Ok(None);
            }
            let mut args = vec![
                "renice".to_string(),
                "-n".into(),
                nice.to_string(),
                "-p".into(),
            ];
            args.extend(denied.iter().map(|p| p.to_string()));
            run_pkexec(&args).map(|_| None)
        });
    }

    /// Efficiency mode: lowest CPU priority and idle I/O class.
    pub fn efficiency(&self, pids: Vec<i32>, on: bool) {
        let ioprio = if on {
            IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT
        } else {
            0
        };
        for &pid in &pids {
            for tid in threads_of(pid) {
                // SAFETY: ioprio_set takes plain integers.
                unsafe {
                    libc::syscall(
                        libc::SYS_ioprio_set,
                        IOPRIO_WHO_PROCESS,
                        tid as libc::c_long,
                        ioprio,
                    );
                }
            }
        }
        self.set_nice(pids, if on { 19 } else { 0 });
    }

    pub fn set_affinity(&self, pid: i32, cpus: Vec<usize>) {
        self.background(move || {
            let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
            for &c in &cpus {
                // SAFETY: c is below CPU_SETSIZE for any realistic machine.
                unsafe { libc::CPU_SET(c, &mut set) };
            }
            let mut denied = false;
            for tid in threads_of(pid) {
                // SAFETY: `set` is a valid cpu_set_t.
                let rc = unsafe {
                    libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set)
                };
                if rc != 0 {
                    if errno() == libc::EPERM {
                        denied = true;
                    } else if errno() != libc::ESRCH {
                        return Err(format!(
                            "Could not set affinity: {}",
                            std::io::Error::last_os_error()
                        ));
                    }
                }
            }
            if denied {
                let list = cpus
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                return run_pkexec(&[
                    "taskset".into(),
                    "-a".into(),
                    "-p".into(),
                    "-c".into(),
                    list,
                    pid.to_string(),
                ])
                .map(|_| None);
            }
            Ok(None)
        });
    }

    pub fn open_location(&self, path: String) {
        self.background(move || {
            let uri = format!("file://{path}");
            let shown = Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=org.freedesktop.FileManager1",
                    "--type=method_call",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1.ShowItems",
                    &format!("array:string:{uri}"),
                    "string:",
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !shown {
                let dir = std::path::Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or(path);
                xdg_open(&dir)?;
            }
            Ok(None)
        });
    }

    pub fn search_online(&self, query: &str) {
        let url = format!("https://duckduckgo.com/?q={}", url_encode(query));
        self.background(move || xdg_open(&url).map(|_| None));
    }

    pub fn open_url(&self, url: &str) {
        let url = url.to_string();
        self.background(move || xdg_open(&url).map(|_| None));
    }

    /// "Run new task".
    pub fn run_task(&self, input: String, admin: bool) {
        self.background(move || {
            let input = input.trim().to_string();
            if input.is_empty() {
                return Ok(None);
            }
            let looks_like_resource = input.contains("://")
                || (std::path::Path::new(&input).exists() && !is_executable(&input));
            let mut cmd = if looks_like_resource && !admin {
                let mut c = Command::new("xdg-open");
                c.arg(&input);
                c
            } else if admin {
                let mut c = Command::new("pkexec");
                c.arg("env");
                for var in [
                    "DISPLAY",
                    "WAYLAND_DISPLAY",
                    "XDG_RUNTIME_DIR",
                    "XAUTHORITY",
                    "DBUS_SESSION_BUS_ADDRESS",
                ] {
                    if let Ok(v) = std::env::var(var) {
                        c.arg(format!("{var}={v}"));
                    }
                }
                c.args(["sh", "-c", &input]);
                c
            } else {
                let mut c = Command::new("sh");
                c.args(["-c", &input]);
                c
            };
            let mut child = cmd
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .map_err(|e| format!("Could not start '{input}': {e}"))?;
            // Reap the child; report quick failures such as "command not found".
            let start = std::time::Instant::now();
            let status = child.wait().map_err(|e| e.to_string())?;
            if !status.success() && start.elapsed().as_secs() < 3 {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut err);
                }
                let err = err.trim();
                return Err(if err.is_empty() {
                    format!("'{input}' exited with {status}")
                } else {
                    err.to_string()
                });
            }
            Ok(None)
        });
    }
}

fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn xdg_open(target: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .and_then(|mut c| c.wait())
        .map(|_| ())
        .map_err(|e| format!("Could not open {target}: {e}"))
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Current CPU affinity of a process, one bool per logical CPU.
pub fn get_affinity(pid: i32, ncpu: usize) -> Vec<bool> {
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is a valid out pointer of the given size.
    let rc =
        unsafe { libc::sched_getaffinity(pid, std::mem::size_of::<libc::cpu_set_t>(), &mut set) };
    (0..ncpu)
        // SAFETY: `set` is initialized; i < CPU_SETSIZE.
        .map(|i| rc != 0 || unsafe { libc::CPU_ISSET(i, &set) })
        .collect()
}

/// Windows-style priority classes mapped to nice values.
pub const PRIORITIES: &[(&str, i32)] = &[
    ("Realtime", -20),
    ("High", -10),
    ("Above normal", -5),
    ("Normal", 0),
    ("Below normal", 5),
    ("Low", 19),
];

pub fn priority_name(nice: i32) -> &'static str {
    match nice {
        i32::MIN..=-15 => "Realtime",
        -14..=-6 => "High",
        -5..=-1 => "Above normal",
        0 => "Normal",
        1..=14 => "Below normal",
        _ => "Low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn actions() -> (Actions, std::sync::mpsc::Receiver<Notice>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Actions::new(tx, egui::Context::default()), rx)
    }

    fn wait_refresh(rx: &std::sync::mpsc::Receiver<Notice>) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(Notice::Refresh) = rx.recv_timeout(Duration::from_millis(100)) {
                return;
            }
        }
        panic!("action did not finish");
    }

    #[test]
    fn end_task_terminates_process() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let (a, rx) = actions();
        a.signal(vec![child.id() as i32], libc::SIGTERM);
        wait_refresh(&rx);
        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    #[test]
    fn end_task_kills_process_ignoring_sigterm() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' TERM; sleep 30"])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let (a, rx) = actions();
        a.signal(vec![child.id() as i32], libc::SIGTERM);
        wait_refresh(&rx);
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn suspend_and_resume() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        let (a, rx) = actions();
        a.signal(vec![pid], libc::SIGSTOP);
        wait_refresh(&rx);
        std::thread::sleep(Duration::from_millis(50));
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        assert!(stat.contains(") T "), "{stat}");
        a.signal(vec![pid], libc::SIGCONT);
        wait_refresh(&rx);
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn lower_priority() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        let (a, rx) = actions();
        a.set_nice(vec![pid], 10);
        wait_refresh(&rx);
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let fields: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..].split(' ').collect();
        assert_eq!(fields[16], "10");
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
