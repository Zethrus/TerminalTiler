#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub mod about_dialog;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod app_chrome;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod appearance;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub mod assets_manager;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub mod command_palette;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub mod companion_dialog;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod context_menu;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod dialog_smoke;
pub(crate) mod icons;
pub mod launch_screen;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub mod layout_tree;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod pane_status;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub mod settings_dialog;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod tab_rename_dialog;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod tile_chrome;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod tile_drag;
#[cfg(target_os = "linux")]
pub mod tile_view;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub(crate) mod title_chrome;
#[cfg(target_os = "linux")]
pub mod web_tile;
#[cfg(target_os = "linux")]
pub mod window;
#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
mod workspace_chrome;
#[cfg(all(target_os = "windows", feature = "windows-gtk-shell"))]
pub mod workspace_preview;
#[cfg(target_os = "linux")]
pub mod workspace_view;
