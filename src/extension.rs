use std::sync::Arc;
use std::time::Duration;

use crate::model::assets::{AgentRoleTemplate, CliSnippet, Runbook};
use crate::model::preset::WorkspacePreset;
use crate::product;

/// Version of the public extension contract consumed by companion applications.
///
/// Increment this when an extension-facing type or behavior changes incompatibly.
pub const CORE_EXTENSION_API_VERSION: u32 = 1;

/// Package version of the Core library that implements the extension contract.
pub const CORE_PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Default)]
pub struct RuntimeOptions {
    pub product: ProductInfo,
    pub companion: Option<Arc<dyn CompanionIntegration>>,
    pub catalog: Option<Arc<dyn CatalogContributionProvider>>,
}

/// Product-specific identity supplied by a Core host or companion build.
///
/// The legacy `app_id` field remains as a compatibility override. New callers
/// should set the platform-specific identifiers instead so Core and companion
/// builds can be installed and run side by side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductIdentity {
    pub display_name: String,
    pub version: String,
    pub app_title: String,
    pub settings_title: String,
    pub settings_summary: String,
    pub settings_copy: Option<String>,
    pub about_copy: Option<String>,
    pub about_extra_copy: Option<String>,
    /// Legacy identifier override used by hosts built against extension API 1.
    pub app_id: Option<String>,
    pub gtk_application_id: String,
    pub windows_app_user_model_id: String,
    pub icon_name: String,
    pub tray_id: String,
    pub tray_title: String,
    pub homepage_url: String,
    pub account_url: String,
    pub support_url: String,
    pub privacy_url: String,
    pub terms_url: String,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub copyright: Option<String>,
}

/// Backward-compatible name retained for existing extension consumers.
pub type ProductInfo = ProductIdentity;

impl ProductIdentity {
    /// Effective GTK application ID, honoring the extension API 1 override.
    pub fn effective_gtk_application_id(&self) -> &str {
        self.app_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.gtk_application_id)
    }

    /// Effective Windows AppUserModelID, honoring the extension API 1 override.
    pub fn effective_windows_app_user_model_id(&self) -> &str {
        self.app_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.windows_app_user_model_id)
    }
}

impl Default for ProductIdentity {
    fn default() -> Self {
        Self {
            display_name: product::PRODUCT_DISPLAY_NAME.to_string(),
            version: product::PRODUCT_VERSION.to_string(),
            app_title: product::PRODUCT_DISPLAY_NAME.to_string(),
            settings_title: product::SETTINGS_DIALOG_TITLE.to_string(),
            settings_summary: product::SETTINGS_SUMMARY_COPY.to_string(),
            settings_copy: None,
            about_copy: Some(product::OPEN_CORE_STATEMENT.to_string()),
            about_extra_copy: None,
            app_id: None,
            gtk_application_id: product::GTK_APPLICATION_ID.to_string(),
            windows_app_user_model_id: product::WINDOWS_APP_USER_MODEL_ID.to_string(),
            icon_name: product::ICON_NAME.to_string(),
            tray_id: product::TRAY_ID.to_string(),
            tray_title: product::TRAY_TITLE.to_string(),
            homepage_url: product::PRODUCT_HOMEPAGE.to_string(),
            account_url: product::PRODUCT_ACCOUNT_URL.to_string(),
            support_url: product::PRODUCT_SUPPORT_URL.to_string(),
            privacy_url: product::PRODUCT_PRIVACY_URL.to_string(),
            terms_url: product::PRODUCT_TERMS_URL.to_string(),
            source_url: Some(product::PRODUCT_SOURCE_URL.to_string()),
            issues_url: Some(product::PRODUCT_ISSUES_URL.to_string()),
            license_name: Some(product::PRODUCT_LICENSE.to_string()),
            license_url: Some(product::PRODUCT_LICENSE_URL.to_string()),
            copyright: Some(product::PRODUCT_COPYRIGHT.to_string()),
        }
    }
}

#[cfg(test)]
mod product_identity_tests {
    use super::ProductIdentity;

    #[test]
    fn default_identity_carries_platform_shell_and_link_contracts() {
        let identity = ProductIdentity::default();

        assert_eq!(identity.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(identity.effective_gtk_application_id(), "app.terminaltiler");
        assert_eq!(
            identity.effective_windows_app_user_model_id(),
            "Zethrus.TerminalTiler"
        );
        assert_eq!(identity.icon_name, "terminaltiler");
        assert_eq!(identity.tray_id, "app.terminaltiler");
        assert!(identity.privacy_url.ends_with("/privacy/"));
        assert!(identity.terms_url.ends_with("/terms/"));
    }

    #[test]
    fn legacy_app_id_remains_the_effective_platform_override() {
        let identity = ProductIdentity {
            app_id: Some("app.example.legacy".to_string()),
            ..ProductIdentity::default()
        };

        assert_eq!(
            identity.effective_gtk_application_id(),
            "app.example.legacy"
        );
        assert_eq!(
            identity.effective_windows_app_user_model_id(),
            "app.example.legacy"
        );
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompanionPanelSnapshot {
    pub title: String,
    pub subtitle: String,
    pub status: CompanionStatus,
    pub account_rows: Vec<CompanionRow>,
    pub sync_rows: Vec<CompanionRow>,
    pub device_rows: Vec<CompanionRow>,
    pub actions: Vec<CompanionAction>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompanionStatus {
    Ok,
    Warning,
    Error,
    Syncing,
    #[default]
    Inactive,
}

impl CompanionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Syncing => "Syncing",
            Self::Inactive => "Inactive",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionRow {
    pub label: String,
    pub value: String,
    pub detail: Option<String>,
}

impl CompanionRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionAction {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub input: Option<CompanionTextInput>,
    pub external_url: Option<String>,
    pub style: CompanionActionStyle,
    pub timeout: Duration,
}

impl CompanionAction {
    pub fn button(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: None,
            input: None,
            external_url: None,
            style: CompanionActionStyle::Normal,
            timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionTextInput {
    pub prompt: String,
    pub placeholder: Option<String>,
    pub secret: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompanionActionStyle {
    #[default]
    Normal,
    Primary,
    Destructive,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompanionActionInput {
    pub text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionActionResult {
    pub message: String,
    pub refresh_scope: CompanionRefreshScope,
}

impl CompanionActionResult {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refresh_scope: CompanionRefreshScope::Panel,
        }
    }

    pub fn with_refresh_scope(mut self, refresh_scope: CompanionRefreshScope) -> Self {
        self.refresh_scope = refresh_scope;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompanionRefreshScope {
    #[default]
    Panel,
    Preferences,
    Presets,
    Assets,
    WorkspaceConfigs,
    Catalog,
    All,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionEvent {
    pub refresh_scope: CompanionRefreshScope,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogTrustMetadata {
    pub read_only: bool,
    pub executable_content: bool,
    pub trusted: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CatalogContributions {
    pub namespace: String,
    pub revision: String,
    pub trust: CatalogTrustMetadata,
    pub presets: Vec<WorkspacePreset>,
    pub role_templates: Vec<AgentRoleTemplate>,
    pub runbooks: Vec<Runbook>,
    pub snippets: Vec<CliSnippet>,
}

pub trait CatalogContributionProvider: Send + Sync {
    /// Returns cached, runtime-only contributions. Implementations must not
    /// perform network or filesystem writes from this method.
    fn contributions(&self) -> Option<CatalogContributions>;
}

pub trait CompanionIntegration: Send + Sync {
    /// Returns cached state only. Network operations belong in `invoke`.
    fn snapshot(&self) -> CompanionPanelSnapshot;

    fn invoke(
        &self,
        action_id: &str,
        input: CompanionActionInput,
    ) -> Result<CompanionActionResult, String>;

    fn drain_events(&self) -> Vec<CompanionEvent> {
        Vec::new()
    }
}
