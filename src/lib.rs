#[cfg(target_os = "linux")]
pub mod app;
pub mod logging;
pub mod model;
pub mod platform;
pub mod companion;
pub mod product;
pub mod services;
pub mod storage;
#[cfg(target_os = "linux")]
pub mod terminal;
pub mod transcript;
#[cfg(target_os = "linux")]
pub mod tray;
#[cfg(target_os = "linux")]
pub mod ui;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub fn run() -> adw::glib::ExitCode {
    app::run()
}

#[cfg(target_os = "windows")]
pub fn run() -> std::process::ExitCode {
    windows::run()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn run() {
    logging::init();
    logging::error("this platform is not supported yet");
    eprintln!("TerminalTiler is not supported on this platform yet.");
}
