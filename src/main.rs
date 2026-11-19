#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}: {}", record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}
static LOGGER: Logger = Logger;

fn main() -> ExitCode {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    if std::env::args().any(|arg| arg == "--scan") {
        match sessiontree::scan_report() {
            Ok(report) => {
                println!("{report}");
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("sessiontree: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    match app() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("sessiontree: {error}");
            ExitCode::FAILURE
        }
    }
}

fn app() -> Result<ExitCode, Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--version") {
        println!("sessiontree {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    platform::set_ui_sets(vec![sessiontree::__nsh_native_module()]);
    let options = platform::AppOptions {
        app_id: "com.hacken.sessiontree".into(),
        title: "Sessiontree".into(),
        size: (1280, 850),
        appearance: platform::Appearance::Dark,
        ..Default::default()
    };
    let result = platform::run_app(
        platform::AppSource::Rust(
            platform::Root::of::<sessiontree::App>().app_id("com.hacken.sessiontree"),
        ),
        options,
    );
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return Ok(result);
    #[cfg(target_os = "linux")]
    return Ok(if result == platform::gtk4::glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    });
}
