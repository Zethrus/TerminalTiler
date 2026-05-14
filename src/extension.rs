use std::sync::Arc;

use crate::product;

#[derive(Clone)]
pub struct RuntimeOptions {
    pub product: ProductInfo,
    pub companion: Option<Arc<dyn CompanionIntegration>>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            product: ProductInfo::default(),
            companion: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductInfo {
    pub display_name: String,
    pub app_title: String,
    pub settings_title: String,
    pub settings_summary: String,
    pub about_extra_copy: Option<String>,
    pub app_id: Option<String>,
}

impl Default for ProductInfo {
    fn default() -> Self {
        Self {
            display_name: product::PRODUCT_DISPLAY_NAME.to_string(),
            app_title: product::PRODUCT_DISPLAY_NAME.to_string(),
            settings_title: product::SETTINGS_DIALOG_TITLE.to_string(),
            settings_summary: product::SETTINGS_SUMMARY_COPY.to_string(),
            about_extra_copy: None,
            app_id: None,
        }
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
    pub refresh: bool,
}

impl CompanionActionResult {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refresh: true,
        }
    }
}

pub trait CompanionIntegration: Send + Sync {
    fn snapshot(&self) -> CompanionPanelSnapshot;

    fn invoke(
        &self,
        action_id: &str,
        input: CompanionActionInput,
    ) -> Result<CompanionActionResult, String>;
}
