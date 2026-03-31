mod app;
mod vt;
mod workspace;
pub mod wsl;

pub fn run() -> std::process::ExitCode {
    app::run()
}
