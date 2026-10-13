//! Human readable PCI device names from the pci.ids database.

use std::path::Path;

const PCI_IDS: &[&str] = &[
    "/usr/share/hwdata/pci.ids",
    "/usr/share/misc/pci.ids",
    "/usr/share/pci.ids",
    "/usr/lib/hwdata/pci.ids",
];

pub struct PciName {
    pub vendor: Option<String>,
    pub device: Option<String>,
    pub subsystem: Option<String>,
}

/// Look up a device in pci.ids.
pub fn lookup(vendor: u32, device: u32, sub_vendor: u32, sub_device: u32) -> PciName {
    let mut out = PciName {
        vendor: None,
        device: None,
        subsystem: None,
    };
    let Some(db) = PCI_IDS.iter().find_map(|p| std::fs::read_to_string(p).ok()) else {
        return out;
    };
    let v = format!("{vendor:04x}  ");
    let d = format!("\t{device:04x}  ");
    let s = format!("\t\t{sub_vendor:04x} {sub_device:04x}  ");
    let mut in_vendor = false;
    let mut in_device = false;
    for line in db.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                break;
            }
            if let Some(name) = line.strip_prefix(&v) {
                in_vendor = true;
                out.vendor = Some(name.to_string());
            }
            continue;
        }
        if !in_vendor {
            continue;
        }
        if !line.starts_with("\t\t") {
            if in_device {
                break;
            }
            if let Some(name) = line.strip_prefix(&d) {
                in_device = true;
                out.device = Some(name.to_string());
            }
        } else if in_device && let Some(name) = line.strip_prefix(&s) {
            out.subsystem = Some(name.to_string());
        }
    }
    out
}

pub fn vendor_short(vendor: u32) -> Option<&'static str> {
    Some(match vendor {
        0x1002 => "AMD",
        0x10de => "NVIDIA",
        0x8086 => "Intel",
        0x14e4 => "Broadcom",
        0x10ec => "Realtek",
        0x168c | 0x17cb => "Qualcomm",
        0x1969 => "Qualcomm Atheros",
        0x1b4b => "Marvell",
        0x15b3 => "Mellanox",
        0x1d6a => "Aquantia",
        0x144d => "Samsung",
        _ => return None,
    })
}

/// Turn "Navi 48 [Radeon RX 9070/9070 XT]" into "AMD Radeon RX 9070/9070 XT".
pub fn pretty_name(vendor: u32, name: &PciName) -> Option<String> {
    let vendor_name = vendor_short(vendor)
        .map(str::to_string)
        .or_else(|| name.vendor.clone())
        .unwrap_or_default();
    let pick = |s: &str| -> String {
        let inner = match (s.find('['), s.rfind(']')) {
            (Some(a), Some(b)) if b > a => &s[a + 1..b],
            _ => s,
        };
        if vendor_name.is_empty()
            || inner
                .to_lowercase()
                .starts_with(&vendor_name.to_lowercase())
        {
            inner.to_string()
        } else {
            format!("{vendor_name} {inner}")
        }
    };
    if let Some(sub) = &name.subsystem
        && sub.contains('[')
    {
        return Some(pick(sub));
    }
    name.device.as_deref().map(pick)
}

/// Read vendor/device ids from a sysfs PCI device directory and produce a nice name.
pub fn name_for_sysfs_device(dev: &Path) -> Option<String> {
    use crate::util::read_hex;
    let vendor = read_hex(dev.join("vendor"))?;
    let device = read_hex(dev.join("device"))?;
    let sv = read_hex(dev.join("subsystem_vendor")).unwrap_or(0);
    let sd = read_hex(dev.join("subsystem_device")).unwrap_or(0);
    pretty_name(vendor, &lookup(vendor, device, sv, sd))
}
