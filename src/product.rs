pub const PRODUCT_DISPLAY_NAME: &str = "TerminalTiler Core";
pub const PRODUCT_VERSION: &str = env!("TERMINALTILER_PACKAGE_VERSION");
pub const PRODUCT_RELEASE_TAG: Option<&str> = option_env!("TERMINALTILER_RELEASE_TAG");
pub const PRODUCT_HOMEPAGE: &str = "https://terminaltiler.app";
pub const PRODUCT_ACCOUNT_URL: &str = "https://terminaltiler.app/account/";
pub const PRODUCT_SUPPORT_URL: &str = "https://terminaltiler.app/support/";
pub const PRODUCT_PRIVACY_URL: &str = "https://terminaltiler.app/privacy/";
pub const PRODUCT_TERMS_URL: &str = "https://terminaltiler.app/terms/";
pub const PRODUCT_SOURCE_URL: &str = "https://github.com/Zethrus/TerminalTiler";
pub const PRODUCT_ISSUES_URL: &str = "https://github.com/Zethrus/TerminalTiler/issues";
pub const PRODUCT_LICENSE: &str = "MIT License";
pub const PRODUCT_LICENSE_URL: &str = "https://github.com/Zethrus/TerminalTiler/blob/main/LICENSE";
pub const PRODUCT_COPYRIGHT: &str = "Copyright (c) 2026 Victor (Zethrus)";
pub const GTK_APPLICATION_ID: &str = "app.terminaltiler";
pub const WINDOWS_APP_USER_MODEL_ID: &str = "Zethrus.TerminalTiler";
pub const ICON_NAME: &str = "terminaltiler";
pub const TRAY_ID: &str = GTK_APPLICATION_ID;
pub const TRAY_TITLE: &str = "TerminalTiler Core";

pub const OPEN_CORE_STATEMENT: &str = "TerminalTiler Core is the public, MIT-licensed foundation of TerminalTiler. TerminalTiler follows an open-core product model: the core app stays public and useful, while this repository stays focused on the public desktop application. The public repository remains the source of truth for the open-source core.";

pub const SETTINGS_DIALOG_TITLE: &str = "TerminalTiler Core Settings";
pub const SETTINGS_SUMMARY_COPY: &str = "MIT-licensed core settings for local workspaces, launch defaults, tray behavior, and shortcuts.";
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub const WINDOWS_SHELL_TITLE: &str = "TerminalTiler Core for Windows";

#[allow(dead_code)]
pub fn display_name_with_version() -> String {
    format!("{PRODUCT_DISPLAY_NAME} v{PRODUCT_VERSION}")
}

pub fn installed_build_label(version: &str) -> String {
    build_label_for(version, PRODUCT_RELEASE_TAG)
}

fn build_label_for(version: &str, release_tag: Option<&str>) -> String {
    let expected_tag = format!("v{version}");
    match release_tag {
        Some(tag) if tag == expected_tag => format!("Release {tag}"),
        _ => format!("Build v{version}"),
    }
}

#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::build_label_for;

    #[test]
    fn build_label_uses_an_exact_release_tag_only() {
        assert_eq!(build_label_for("0.3.2", Some("v0.3.2")), "Release v0.3.2");
        assert_eq!(build_label_for("0.3.2", None), "Build v0.3.2");
        assert_eq!(build_label_for("0.3.2", Some("v0.3.1")), "Build v0.3.2");
        assert_eq!(
            build_label_for("0.3.2", Some("release-0.3.2")),
            "Build v0.3.2"
        );
    }
}
