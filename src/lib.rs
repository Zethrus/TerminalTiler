#[cfg(target_os = "linux")]
mod app;
mod dropped_paths;
mod logging;
mod model;
mod platform;
mod product;
mod services;
mod storage;
#[cfg(target_os = "linux")]
mod terminal;
mod transcript;
#[cfg(target_os = "linux")]
mod tray;
#[cfg(target_os = "linux")]
mod ui;
#[cfg(target_os = "windows")]
mod windows;

/// Public entrypoint for launching the TerminalTiler application.
///
/// This keeps the public Core package reusable from external binaries, including
/// private external applications that need to embed the open-core app without
/// introducing any external-specific dependency back into this repository.
#[cfg(target_os = "linux")]
pub fn run() -> adw::glib::ExitCode {
    app::run()
}

/// Public entrypoint for launching the TerminalTiler application.
#[cfg(target_os = "windows")]
pub fn run() -> std::process::ExitCode {
    windows::run()
}

/// Public entrypoint for launching the TerminalTiler application.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn run() {
    logging::init();
    logging::error("this platform is not supported yet");
    eprintln!("TerminalTiler is not supported on this platform yet.");
}
