# Manager of Tasks

A task manager for Linux inspired by the **Windows 11 Task Manager**, written in Rust with [egui](https://github.com/emilk/egui).

Processes grouped into *Apps / Background processes / System processes* with heat-map columns, live Performance graphs for CPU, memory, disks, network and **GPUs (AMD, NVIDIA and Intel)**, plus App history, Startup apps, Users, Details and Services pages.

## Features

| Page | What it does |
| --- | --- |
| **Processes** | Apps (each launched application with its child processes), background processes and system processes. CPU, Memory, Disk, GPU and GPU engine columns with heat-map shading and totals in the header. End task, force end, suspend/resume, Efficiency mode, open file location, search online, properties. |
| **Performance** | Sidebar with live thumbnails. CPU (overall or per-logical-processor graphs, kernel times, speed, cores, caches, uptime), Memory (usage graph, composition bar, committed, cached, swap), every disk (active time, transfer rate, response time), every network adapter (throughput, IPv4/IPv6, link speed, SSID) and every GPU (3D/Compute/Copy/Video engine graphs, dedicated and shared memory, temperature, clocks, power, driver). |
| **App history** | Accumulated CPU time, disk I/O and GPU time per app, stored across sessions. |
| **Startup apps** | XDG autostart entries; enable or disable them like on Windows. Shows the last firmware ("BIOS") time. |
| **Users** | Resource usage per signed-in user, expandable to their processes. |
| **Details** | Every process with 20+ selectable columns (right-click the header). Set priority, set CPU affinity, end process tree, properties. |
| **Services** | systemd system and user services: start, stop, restart, enable/disable at startup. |
| **Settings** | Theme (light / dark / system), real-time update speed (High / Normal / Low / Paused), default start page, always on top, and more. |

Other details that follow Windows:

- Search box in the title bar filters every page by name or PID (<kbd>Ctrl</kbd>+<kbd>F</kbd>).
- Collapsible navigation pane, Fluent styling, and your desktop's accent color (KDE).
- <kbd>Delete</kbd> ends the selected task, <kbd>F5</kbd> refreshes, <kbd>Ctrl</kbd>+<kbd>Tab</kbd> switches pages.
- Actions that need more privileges (ending a root process, raising priority) ask for your password through polkit, the Linux equivalent of "Run as administrator".
- "Run new task" can run a command or open a file, folder or URL, optionally as administrator.

## GPU support

| Vendor | Overall utilization & memory | Engines | Per-process GPU % |
| --- | --- | --- | --- |
| AMD (amdgpu) | ✅ sysfs (`gpu_busy_percent`, VRAM, GTT) | ✅ 3D, Compute, Copy, Video | ✅ DRM fdinfo |
| Intel (i915 / xe) | ✅ from engine activity | ✅ 3D, Copy, Video, Video Processing | ✅ DRM fdinfo |
| NVIDIA (proprietary driver) | ✅ NVML (loaded at runtime) | ✅ 3D, Memory, Encode, Decode | ✅ NVML |

Temperature, clocks, power draw and fan speed are shown when the driver exposes them. NVML is loaded with `dlopen`, so the app works fine on machines without NVIDIA hardware.

## Install

### Quick install (x86_64)

```sh
curl -fsSL https://raw.githubusercontent.com/SilverBoi78/manager-of-tasks/main/scripts/get.sh | sh
```

This downloads the latest [release](https://github.com/SilverBoi78/manager-of-tasks/releases), checks its SHA-256 checksum and installs it to `~/.local` with an app menu entry. Nothing is installed system-wide and no `sudo` is needed. You can [read the script](scripts/get.sh) before running it.

To uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/SilverBoi78/manager-of-tasks/main/scripts/get.sh | sh -s -- --uninstall
```

The prebuilt binary runs on distros with glibc 2.35 or newer: Ubuntu 22.04+, Debian 12+, Fedora 36+, Linux Mint 21+, openSUSE Tumbleweed, Arch and its derivatives, and similar.

### Manual download

Download `manager-of-tasks-x86_64-linux.tar.gz` from the [Releases page](https://github.com/SilverBoi78/manager-of-tasks/releases), then:

```sh
tar xzf manager-of-tasks-x86_64-linux.tar.gz
./manager-of-tasks/manager-of-tasks              # run it directly, or
./manager-of-tasks/scripts/install.sh            # install to ~/.local
```

### Build from source

Needed for other CPU architectures (ARM) and musl distros (Alpine). You need a Rust toolchain (1.95 or newer) from [rustup.rs](https://rustup.rs); distro-packaged Rust is often too old.

```sh
git clone https://github.com/SilverBoi78/manager-of-tasks.git
cd manager-of-tasks
./scripts/install.sh            # builds and installs to ~/.local (binary, menu entry and icon)
```

Uninstall with `./scripts/install.sh --uninstall`. To install system-wide, run `sudo PREFIX=/usr/local ./scripts/install.sh`. Or just run it without installing: `cargo run --release`.

On Debian/Ubuntu, building also needs:

```sh
sudo apt install build-essential libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

### Open it with Ctrl+Shift+Esc

- **KDE Plasma:** System Settings → Keyboard → Shortcuts → Add New → Application → *Manager of Tasks*, then assign <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Esc</kbd>.
- **GNOME:** Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts → command `manager-of-tasks`.

## Command line

```
manager-of-tasks [--page <PAGE>] [--system-info] [--version]
```

`--page` opens a page directly: `processes`, `performance`, `app-history`, `startup`, `users`, `details`, `services`, `settings`, or a Performance view such as `cpu`, `memory`, `disk0`, `net0` or `gpu0`. For example, `manager-of-tasks --page gpu` opens straight to the GPU graphs.

## Compatibility

Manager of Tasks reads standard kernel interfaces (`/proc`, `/sys`), so it should work on any modern distro, on both Wayland and X11. It's still young, though, and has only been tested on a few machines:

| Tested | Hardware / setup |
| --- | --- |
| ✅ Works | CachyOS, KDE Plasma 6 (Wayland), Intel Core i5-13600K, AMD Radeon RX 9070 XT |
| ⚠️ Expected to work, untested | GNOME and other desktops, X11 sessions, NVIDIA (proprietary driver), Intel graphics, laptops with two GPUs |
| ⚠️ Partial | Distros without systemd: no Services page, less precise app grouping |
| ❌ Not yet | GPUs that aren't PCI devices (Raspberry Pi and other ARM boards); prebuilt binaries for ARM |

If you try it, please [report how it went](https://github.com/SilverBoi78/manager-of-tasks/issues/new/choose), even if it works fine. **Settings → Report a problem → Copy system info** (or `manager-of-tasks --system-info`) copies everything we need.

## How it maps Windows concepts to Linux

- **Apps vs. background processes.** Wayland doesn't let one app see other apps' windows. Instead, Manager of Tasks uses the systemd scopes that KDE Plasma and GNOME create for every launched application (`app-<desktop-id>@….service`) and matches them to `.desktop` files for names and icons. Apps started from autostart or D-Bus count as background processes. On systems without these scopes it falls back to matching executables against installed desktop entries.
- **Memory** on the Processes page is private (anonymous) resident memory, like Windows' "active private working set".
- **Efficiency mode** sets the process to the lowest CPU priority (nice 19) and idle I/O priority.
- **Base priority** maps Windows priority classes to nice values (Realtime -20, High -10, Above normal -5, Normal 0, Below normal 5, Low 19).
- **Handles** shows the number of open file handles on the system.
- **Startup apps** are XDG autostart entries. Disabling one writes a `Hidden=true` override to `~/.config/autostart`, which is the standard, reversible way.

## Known limitations

- Per-process **network** usage isn't shown; Linux only exposes it with root/eBPF.
- Memory **speed, slots and form factor** need root access to DMI tables, so they're left out.
- **Startup impact** isn't measured.
- **Always on top** depends on the compositor; KDE and GNOME on Wayland may ignore it.
- Disk activity and GPU usage of processes owned by *other* users can't be read without root.

## Where data is stored

- Settings: `~/.config/manager-of-tasks/settings.json`
- App history: `~/.local/share/manager-of-tasks/app_history.json`

## Contributing

Issues and pull requests are welcome. Before sending a PR, please run:

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

The code is organized as:

- `src/sys/` collects system data on a background thread (CPU, memory, disks, network, GPUs, processes, services, startup entries).
- `src/ui/` holds one module per page plus shared widgets (graphs, tables, buttons).
- `src/actions.rs` handles ending, suspending and reprioritizing processes, with polkit fallback.
- `src/theme.rs` defines the Windows 11 look.

## License

[MIT](LICENSE).

Manager of Tasks is an independent project. It is not affiliated with, endorsed by, or sponsored by Microsoft, and it contains no Microsoft code, images, icons or fonts. Microsoft and Windows are trademarks of the Microsoft group of companies.
