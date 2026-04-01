pub mod alert_center;
mod app;
pub mod assets_manager;
pub mod command_palette;
pub mod runbook_dialog;
pub mod transcript_viewer;
mod vt;
mod workspace;
pub mod wsl;

pub fn run() -> std::process::ExitCode {
    app::run()
}
