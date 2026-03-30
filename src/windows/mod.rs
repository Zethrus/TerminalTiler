mod app;
#[cfg(target_os = "windows")]
mod workspace;
pub mod wsl;

pub fn run() -> std::process::ExitCode {
    app::run()
}
