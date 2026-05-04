#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod app;
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

#[cfg(target_os = "linux")]
fn main() -> adw::glib::ExitCode {
    app::run()
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    windows::run()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    logging::init();
    logging::error("this platform is not supported yet");
    eprintln!("TerminalTiler is not supported on this platform yet.");
}
