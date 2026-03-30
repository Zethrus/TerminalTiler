mod app;
#[cfg(any(target_os = "windows", test))]
mod vt;
#[cfg(target_os = "windows")]
mod workspace;
pub mod wsl;

pub fn run() -> std::process::ExitCode {
    app::run()
}
