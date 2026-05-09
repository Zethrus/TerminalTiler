#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn main() -> adw::glib::ExitCode {
    terminaltiler::run()
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    terminaltiler::run()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    terminaltiler::run()
}
