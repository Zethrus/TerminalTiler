pub mod alert_center;
#[cfg(any(not(feature = "windows-gtk-shell"), feature = "windows-win32-shell"))]
mod app;
pub mod assets_manager;
pub mod command_palette;
#[cfg(all(feature = "windows-gtk-shell", not(feature = "windows-win32-shell")))]
mod gtk_app;
mod launcher_editor;
mod restore_prompt;
pub mod runbook_dialog;
pub mod shortcut_capture;
pub mod theme;
pub mod transcript_viewer;
mod vt;
mod win32_helpers;
mod workspace;
pub mod wsl;

pub fn run() -> std::process::ExitCode {
    #[cfg(all(feature = "windows-gtk-shell", not(feature = "windows-win32-shell")))]
    {
        return gtk_app::run();
    }
    #[cfg(any(not(feature = "windows-gtk-shell"), feature = "windows-win32-shell"))]
    app::run()
}

pub fn run_with_options(options: crate::extension::RuntimeOptions) -> std::process::ExitCode {
    #[cfg(all(feature = "windows-gtk-shell", not(feature = "windows-win32-shell")))]
    {
        return gtk_app::run_with_options(options);
    }
    #[cfg(any(not(feature = "windows-gtk-shell"), feature = "windows-win32-shell"))]
    app::run_with_options(options)
}
