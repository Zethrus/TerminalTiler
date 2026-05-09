use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductEdition {
    Core,
    companion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProCapability {
    CloudSync,
    TeamSharing,
    ManagedBilling,
    PremiumThemePacks,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProRuntimeDescriptor {
    pub edition: ProductEdition,
    pub capabilities: Vec<ProCapability>,
    pub account_label: Option<String>,
    pub sync_status_label: Option<String>,
}

impl ProRuntimeDescriptor {
    pub fn core() -> Self {
        Self {
            edition: ProductEdition::Core,
            capabilities: Vec::new(),
            account_label: None,
            sync_status_label: None,
        }
    }

    pub fn companion(capabilities: Vec<ProCapability>) -> Self {
        Self {
            edition: ProductEdition::companion,
            capabilities,
            account_label: None,
            sync_status_label: None,
        }
    }

    pub fn has(&self, capability: ProCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

pub trait ProIntegration {
    fn descriptor(&self) -> ProRuntimeDescriptor;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CoreOnlyIntegration;

impl ProIntegration for CoreOnlyIntegration {
    fn descriptor(&self) -> ProRuntimeDescriptor {
        ProRuntimeDescriptor::core()
    }
}

pub fn compiled_edition() -> ProductEdition {
    if cfg!(feature = "companion") {
        ProductEdition::companion
    } else {
        ProductEdition::Core
    }
}

pub fn compiled_descriptor() -> ProRuntimeDescriptor {
    if cfg!(feature = "companion") {
        ProRuntimeDescriptor::companion(vec![
            ProCapability::CloudSync,
            ProCapability::TeamSharing,
            ProCapability::ManagedBilling,
            ProCapability::PremiumThemePacks,
        ])
    } else {
        ProRuntimeDescriptor::core()
    }
}
