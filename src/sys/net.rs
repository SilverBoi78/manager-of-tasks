//! Network adapter throughput from /proc/net/dev.

use super::Series;
use crate::util::{read, read_i64, read_trim};
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct NetInfo {
    pub iface: String,
    /// "Ethernet", "Wi-Fi" or "VPN".
    pub kind: String,
    pub adapter: String,
    pub ssid: Option<String>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub link_mbps: Option<u64>,
    pub mac: String,
}

#[derive(Clone, Default)]
pub struct NetStats {
    pub info: Arc<NetInfo>,
    /// Bits per second.
    pub send_bps: f64,
    pub recv_bps: f64,
    pub send_history: Series,
    pub recv_history: Series,
}

struct Entry {
    stats: NetStats,
    prev: Option<(u64, u64)>,
}

pub struct NetSampler {
    entries: Vec<Entry>,
    last: Instant,
    tick: u64,
}

impl NetSampler {
    pub fn new() -> Self {
        let mut s = Self {
            entries: Vec::new(),
            last: Instant::now(),
            tick: 0,
        };
        s.refresh_devices();
        s
    }

    fn refresh_devices(&mut self) {
        let addrs = interface_addresses();
        let mut entries = Vec::new();
        for iface in list_interfaces() {
            let old = self
                .entries
                .iter()
                .position(|e| e.stats.info.iface == iface)
                .map(|i| self.entries.swap_remove(i));
            let ssid_cached = old.as_ref().and_then(|e| e.stats.info.ssid.clone());
            let refresh_ssid = old.is_none() || self.tick.is_multiple_of(30);
            let info = Arc::new(net_info(&iface, &addrs, ssid_cached, refresh_ssid));
            entries.push(match old {
                Some(mut e) => {
                    e.stats.info = info;
                    e
                }
                None => Entry {
                    stats: NetStats {
                        info,
                        ..Default::default()
                    },
                    prev: None,
                },
            });
        }
        self.entries = entries;
    }

    pub fn sample(&mut self) -> Vec<NetStats> {
        self.tick += 1;
        if self.tick.is_multiple_of(10) {
            self.refresh_devices();
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;

        let text = read("/proc/net/dev").unwrap_or_default();
        let mut counters: HashMap<&str, (u64, u64)> = HashMap::new();
        for line in text.lines().skip(2) {
            let Some((name, rest)) = line.split_once(':') else {
                continue;
            };
            let f: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            if f.len() >= 9 {
                counters.insert(name.trim(), (f[0], f[8]));
            }
        }
        for e in &mut self.entries {
            let Some(&(rx, tx)) = counters.get(e.stats.info.iface.as_str()) else {
                continue;
            };
            if let (Some((prx, ptx)), true) = (e.prev, dt > 0.0) {
                e.stats.recv_bps = rx.saturating_sub(prx) as f64 * 8.0 / dt;
                e.stats.send_bps = tx.saturating_sub(ptx) as f64 * 8.0 / dt;
            }
            e.prev = Some((rx, tx));
            e.stats.recv_history.push(e.stats.recv_bps as f32);
            e.stats.send_history.push(e.stats.send_bps as f32);
        }
        self.entries.iter().map(|e| e.stats.clone()).collect()
    }
}

fn is_vpn(name: &str) -> bool {
    ["wg", "tun", "tap", "tailscale", "ppp", "nordlynx", "proton"]
        .iter()
        .any(|p| name.starts_with(p))
}

fn list_interfaces() -> Vec<String> {
    let mut physical = Vec::new();
    let mut up = Vec::new();
    for e in std::fs::read_dir("/sys/class/net")
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }
        let base = e.path();
        let state = read_trim(base.join("operstate")).unwrap_or_default();
        let is_up = state == "up" || state == "dormant" || state == "unknown";
        if base.join("device").exists() {
            physical.push((name.clone(), is_up));
        } else if is_vpn(&name) && is_up {
            up.push(name);
        }
    }
    physical.sort();
    let any_up = physical.iter().any(|(_, u)| *u);
    let mut out: Vec<String> = physical
        .into_iter()
        .filter(|(_, u)| *u || !any_up)
        .map(|(n, _)| n)
        .collect();
    up.sort();
    out.extend(up);
    out
}

fn net_info(
    iface: &str,
    addrs: &HashMap<String, (Vec<String>, Vec<String>)>,
    ssid_cached: Option<String>,
    refresh_ssid: bool,
) -> NetInfo {
    let base = PathBuf::from("/sys/class/net").join(iface);
    let wireless = base.join("wireless").exists() || base.join("phy80211").exists();
    let kind = if wireless {
        "Wi-Fi"
    } else if is_vpn(iface) {
        "VPN"
    } else {
        "Ethernet"
    }
    .to_string();
    let dev = base.join("device");
    let adapter = super::pci::name_for_sysfs_device(&dev)
        .or_else(|| read_trim(dev.join("../product")))
        .or_else(|| {
            std::fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        })
        .unwrap_or_else(|| iface.to_string());
    let link_mbps = read_i64(base.join("speed"))
        .filter(|&s| s > 0)
        .map(|s| s as u64);
    let ssid = if wireless {
        if refresh_ssid {
            query_ssid(iface).or(ssid_cached)
        } else {
            ssid_cached
        }
    } else {
        None
    };
    let (ipv4, ipv6) = addrs.get(iface).cloned().unwrap_or_default();
    NetInfo {
        iface: iface.to_string(),
        kind,
        adapter,
        ssid,
        ipv4,
        ipv6,
        link_mbps,
        mac: read_trim(base.join("address")).unwrap_or_default(),
    }
}

fn query_ssid(iface: &str) -> Option<String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-g", "GENERAL.CONNECTION", "device", "show", iface])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// IPv4 and IPv6 addresses per interface via getifaddrs(3).
fn interface_addresses() -> HashMap<String, (Vec<String>, Vec<String>)> {
    let mut out: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `ifap` with a linked list we free with freeifaddrs.
    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return out;
    }
    let mut cur = ifap;
    while !cur.is_null() {
        // SAFETY: `cur` is a valid node of the list returned by getifaddrs.
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
            continue;
        }
        // SAFETY: ifa_name is a valid C string.
        let name = unsafe { CStr::from_ptr(ifa.ifa_name) }
            .to_string_lossy()
            .to_string();
        // SAFETY: ifa_addr is non-null; we check the family before casting.
        let family = unsafe { (*ifa.ifa_addr).sa_family } as i32;
        let entry = out.entry(name).or_default();
        if family == libc::AF_INET {
            // SAFETY: family is AF_INET so the address is a sockaddr_in.
            let sa = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sa.sin_addr.s_addr));
            entry.0.push(ip.to_string());
        } else if family == libc::AF_INET6 {
            // SAFETY: family is AF_INET6 so the address is a sockaddr_in6.
            let sa = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
            let ip = std::net::Ipv6Addr::from(sa.sin6_addr.s6_addr);
            entry.1.push(ip.to_string());
        }
    }
    // SAFETY: `ifap` was returned by getifaddrs.
    unsafe { libc::freeifaddrs(ifap) };
    out
}
