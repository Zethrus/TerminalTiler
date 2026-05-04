pub const PRODUCT_DISPLAY_NAME: &str = "TerminalTiler Core";
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PRODUCT_HOMEPAGE: &str = "https://terminaltiler.app";
pub const PRODUCT_SOURCE_URL: &str = "https://github.com/Zethrus/TerminalTiler";
pub const PRODUCT_ISSUES_URL: &str = "https://github.com/Zethrus/TerminalTiler/issues";
pub const PRODUCT_LICENSE: &str = "MIT License";
pub const PRODUCT_COPYRIGHT: &str = "Copyright (c) 2026 Zethrus Victor";

pub const OPEN_CORE_STATEMENT: &str = "TerminalTiler Core is the public, MIT-licensed foundation of TerminalTiler. TerminalTiler follows an open-core product model: the core app stays public and useful, while future Pro offerings may add paid workflow packs, convenience features, official support, or private extensions. The public repository remains the source of truth for the open-source core.";

pub const SETTINGS_DIALOG_TITLE: &str = "TerminalTiler Core Settings";
pub const SETTINGS_SUMMARY_COPY: &str = "MIT-licensed core settings for local workspaces, launch defaults, tray behavior, and shortcuts.";
#[cfg(target_os = "windows")]
pub const WINDOWS_SHELL_TITLE: &str = "TerminalTiler Core for Windows";

pub fn display_name_with_version() -> String {
    format!("{PRODUCT_DISPLAY_NAME} v{PRODUCT_VERSION}")
}

pub fn about_title() -> String {
    format!("About {PRODUCT_DISPLAY_NAME}")
}

#[cfg(target_os = "windows")]
pub fn about_body() -> String {
    format!(
        "{}\n{}\n{}\n\n{}\n\nWebsite: {}\nSource: {}\nIssues: {}",
        display_name_with_version(),
        PRODUCT_COPYRIGHT,
        PRODUCT_LICENSE,
        OPEN_CORE_STATEMENT,
        PRODUCT_HOMEPAGE,
        PRODUCT_SOURCE_URL,
        PRODUCT_ISSUES_URL
    )
}
