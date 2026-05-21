//! Shared GTK visual contract used by the Linux shell and the Windows GTK shell.
//!
//! Linux remains the canonical implementation.  The Windows GTK frontend uses
//! these same resources and class names so visual parity work cannot drift into a
//! second design language while the platform runtime backends stay separate.

/// Canonical GTK/libadwaita stylesheet shared by every GTK shell.
pub const STYLE_CSS: &str = include_str!("../../resources/style.css");

/// Resource paths that Windows packaging must stage next to the executable.
pub const WINDOWS_GTK_RESOURCE_PAYLOAD: &[&str] = &[
    "resources/style.css",
    "resources/terminaltiler.svg",
    "resources/hover-icons/arrow-back.svg",
    "resources/hover-icons/arrow-narrow-right.svg",
    "resources/hover-icons/checked.svg",
    "resources/hover-icons/copy.svg",
    "resources/hover-icons/external-link.svg",
    "resources/hover-icons/layout-dashboard.svg",
    "resources/hover-icons/player.svg",
    "resources/hover-icons/refresh.svg",
    "resources/hover-icons/save.svg",
    "resources/hover-icons/send-horizontal.svg",
    "resources/hover-icons/terminal.svg",
    "resources/hover-icons/trash.svg",
    "resources/hover-icons/triangle-alert.svg",
    "resources/hover-icons/x.svg",
];

/// CSS classes that define the parity contract between Ubuntu GTK and Windows GTK.
/// Keep this list narrow and tied to visible shell surfaces rather than every
/// helper class in the stylesheet.
pub const SHARED_VISUAL_CONTRACT_CLASSES: &[&str] = &[
    "window-shell",
    "theme-light",
    "theme-dark",
    "profile-comfortable",
    "profile-standard",
    "profile-compact",
    "launch-shell",
    "launch-stage",
    "launch-stage-clamp",
    "launch-dashboard",
    "launch-wizard-shell",
    "saved-workspace-card",
    "saved-workspace-actions",
    "wizard-step-chip",
    "app-tab-strip",
    "app-tab",
    "workspace-summary",
    "terminal-card",
    "terminal-header",
    "terminal-frame",
    "terminal-surface",
    "web-tile-frame",
    "primary-cta-button",
    "secondary-button",
    "ghost-link-button",
    "surface-button",
    "destructive-button",
];

/// Internal adapter boundaries that platform frontends must provide while
/// sharing the GTK chrome.  Linux implementations remain VTE/WebKit-backed;
/// Windows implementations remain ConPTY/WebView2-backed.
pub const PLATFORM_RUNTIME_ADAPTERS: &[&str] = &[
    "terminal-pane",
    "web-pane",
    "workspace-runtime-actions",
    "runtime-capability-checks",
];

#[cfg(any(
    target_os = "linux",
    all(target_os = "windows", feature = "windows-gtk-shell")
))]
pub fn load_css_for_default_display() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE_CSS);

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn css_contains_visual_contract() -> bool {
    SHARED_VISUAL_CONTRACT_CLASSES
        .iter()
        .all(|class_name| STYLE_CSS.contains(&format!(".{class_name}")))
}
