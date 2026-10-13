//! Manager of Tasks — a task manager for Linux inspired by the Windows 11 Task Manager.

mod actions;
mod app;
mod settings;
mod sys;
mod theme;
mod ui;
mod util;

fn main() -> eframe::Result {
    let settings = settings::Settings::load();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Manager of Tasks {}\n\nUsage: manager-of-tasks [OPTIONS]\n\n  --page <PAGE>    Open on a page: processes, performance, app-history, startup,\n                   users, details, services, settings, cpu, memory, diskN, netN, gpuN\n  --system-info    Print a system summary for bug reports\n  -V, --version    Print the version\n  -h, --help       Print this help",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--system-info") {
        print!("{}", sys::report::system_info());
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("manager-of-tasks {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let page_arg = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1));
    let perf_item = page_arg.and_then(|p| ui::performance::Item::from_arg(p));
    let start_page = page_arg
        .and_then(|p| settings::Page::from_arg(p))
        .or(perf_item.map(|_| settings::Page::Performance));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Manager of Tasks")
            .with_app_id("manager-of-tasks")
            .with_inner_size([1060.0, 720.0])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(std::sync::Arc::new(icon())),
        ..Default::default()
    };
    eframe::run_native(
        "Manager of Tasks",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, settings, start_page, perf_item)))),
    )
}

/// Window icon: a blue rounded square with a white pulse line, drawn in code.
fn icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    let pts = [
        (0.0, 0.6),
        (0.25, 0.6),
        (0.4, 0.2),
        (0.6, 0.85),
        (0.75, 0.45),
        (1.0, 0.45),
    ];
    let (m, s) = (N as f32 * 0.18, N as f32 * 0.64);
    let pts: Vec<(f32, f32)> = pts.iter().map(|(x, y)| (m + x * s, m + y * s)).collect();
    let r = N as f32 * 0.22;
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded rectangle coverage.
            let dx = (r - fx).max(fx - (N as f32 - r)).max(0.0);
            let dy = (r - fy).max(fy - (N as f32 - r)).max(0.0);
            if (dx * dx + dy * dy).sqrt() > r {
                continue;
            }
            let mut c = [0x00u8, 0x67, 0xc0, 0xff];
            // Distance to the pulse polyline.
            let d = pts
                .windows(2)
                .map(|w| seg_dist((fx, fy), w[0], w[1]))
                .fold(f32::MAX, f32::min);
            if d < N as f32 * 0.05 {
                c = [0xff, 0xff, 0xff, 0xff];
            }
            rgba[(y * N + x) * 4..(y * N + x) * 4 + 4].copy_from_slice(&c);
        }
    }
    egui::IconData {
        rgba,
        width: N as u32,
        height: N as u32,
    }
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * vx, a.1 + t * vy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}
