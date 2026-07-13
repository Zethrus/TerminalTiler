#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn print_runtime_capabilities() -> bool {
    if std::env::args_os().any(|argument| argument == "--runtime-capabilities") {
        println!("{}", terminaltiler::extension::runtime_capabilities_json());
        true
    } else {
        false
    }
}

#[cfg(target_os = "linux")]
fn main() -> adw::glib::ExitCode {
    if print_runtime_capabilities() {
        adw::glib::ExitCode::SUCCESS
    } else {
        terminaltiler::run()
    }
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    if print_runtime_capabilities() {
        std::process::ExitCode::SUCCESS
    } else {
        terminaltiler::run()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    if !print_runtime_capabilities() {
        terminaltiler::run()
    }
}
