//! Small helpers: number formatting and file reading.

use std::fs;
use std::path::Path;

/// Read a file to a string, returning `None` on any error.
pub fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Read a file and return its trimmed contents.
pub fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    read(path).map(|s| s.trim().to_string())
}

/// Read a file containing a single integer.
pub fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trim(path)?.parse().ok()
}

pub fn read_i64(path: impl AsRef<Path>) -> Option<i64> {
    read_trim(path)?.parse().ok()
}

/// Read a hex value such as `0x1002`.
pub fn read_hex(path: impl AsRef<Path>) -> Option<u32> {
    let s = read_trim(path)?;
    u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

const KB: f64 = 1024.0;
const MB: f64 = KB * 1024.0;
const GB: f64 = MB * 1024.0;
const TB: f64 = GB * 1024.0;

/// "1.2 GB" style, like the Performance page.
pub fn bytes(b: u64) -> String {
    let b = b as f64;
    if b >= TB {
        format!("{:.1} TB", b / TB)
    } else if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{b:.0} B")
    }
}

/// Memory column on the Processes tab: always in MB with one decimal ("1,234.5 MB").
pub fn mem_mb(b: u64) -> String {
    let mb = b as f64 / MB;
    format!("{} MB", group_thousands_f(mb, 1))
}

/// Details tab memory: "123,456 K".
pub fn mem_k(b: u64) -> String {
    format!("{} K", group_thousands(b / 1024))
}

/// Disk rate on the Processes tab: "0.1 MB/s".
pub fn disk_rate_mb(bps: f64) -> String {
    format!("{:.1} MB/s", bps / MB)
}

/// Rate with an automatic unit: "512 KB/s", "12.3 MB/s".
pub fn rate(bps: f64) -> String {
    if bps >= GB {
        format!("{:.1} GB/s", bps / GB)
    } else if bps >= MB {
        format!("{:.1} MB/s", bps / MB)
    } else {
        format!("{:.0} KB/s", bps / KB)
    }
}

/// Network throughput in bits per second: "8.0 Kbps", "1.2 Mbps".
pub fn bits(bits_per_sec: f64) -> String {
    let b = bits_per_sec;
    if b >= 1e9 {
        format!("{:.1} Gbps", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} Mbps", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.1} Kbps", b / 1e3)
    } else {
        format!("{b:.0} bps")
    }
}

/// Percentage with one decimal, like the Processes tab: "0.3%".
pub fn pct1(p: f32) -> String {
    format!("{p:.1}%")
}

/// Uptime: "d:hh:mm:ss".
pub fn uptime(secs: f64) -> String {
    let s = secs as u64;
    format!(
        "{}:{:02}:{:02}:{:02}",
        s / 86400,
        (s / 3600) % 24,
        (s / 60) % 60,
        s % 60
    )
}

/// CPU time: "h:mm:ss".
pub fn cpu_time(secs: f64) -> String {
    let s = secs as u64;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Frequency given in MHz: "3.45 GHz".
pub fn ghz(mhz: f64) -> String {
    if mhz >= 1000.0 {
        format!("{:.2} GHz", mhz / 1000.0)
    } else {
        format!("{mhz:.0} MHz")
    }
}

pub fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn group_thousands_f(v: f64, decimals: usize) -> String {
    let int = v.trunc() as u64;
    let frac = format!("{:.*}", decimals, v.fract());
    // `frac` is like "0.5" (or "1.0" after rounding up).
    if let Some(rest) = frac.strip_prefix('1') {
        return format!("{}{rest}", group_thousands(int + 1));
    }
    format!("{}{}", group_thousands(int), frac.trim_start_matches('0'))
}

/// Choose a "nice" upper bound for an auto-scaled graph.
pub fn nice_ceiling(v: f64) -> f64 {
    if v <= 0.0 {
        return 1.0;
    }
    let exp = v.log10().floor();
    let base = 10f64.powf(exp);
    for m in [1.0, 2.0, 5.0, 10.0] {
        if v <= m * base {
            return m * base;
        }
    }
    10.0 * base
}

/// Undo systemd's `\xNN` escaping in unit names.
pub fn systemd_unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1] == b'x'
            && let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16)
        {
            out.push(v);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Local date and time, e.g. "10/12/2026 8:19 PM".
pub fn local_datetime(unix: u64) -> String {
    let t = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return unix.to_string();
    }
    let (h12, ampm) = match tm.tm_hour {
        0 => (12, "AM"),
        1..=11 => (tm.tm_hour, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!(
        "{}/{}/{} {}:{:02} {}",
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_year + 1900,
        h12,
        tm.tm_min,
        ampm
    )
}

/// Local date only, e.g. "10/12/2026".
pub fn local_date(unix: u64) -> String {
    local_datetime(unix)
        .split(' ')
        .next()
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1000), "1,000");
        assert_eq!(group_thousands(1234567), "1,234,567");
        assert_eq!(mem_mb(1024 * 1024 * 1234 + 1024 * 512), "1,234.5 MB");
        assert_eq!(mem_mb(1024 * 1024 * 999 + 1024 * 1023), "1,000.0 MB");
    }

    #[test]
    fn unescape() {
        assert_eq!(systemd_unescape(r"google\x2dchrome"), "google-chrome");
        assert_eq!(systemd_unescape("plain"), "plain");
    }

    #[test]
    fn nice() {
        assert_eq!(nice_ceiling(0.7), 1.0);
        assert_eq!(nice_ceiling(130.0), 200.0);
        assert_eq!(nice_ceiling(4000.0), 5000.0);
    }
}
