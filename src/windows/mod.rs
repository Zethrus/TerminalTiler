mod app;
pub mod assets_manager;
pub mod command_palette;
pub mod runbook_dialog;
mod vt;
mod workspace;
pub mod wsl;

pub fn run() -> std::process::ExitCode {
    app::run()
}
