use serde::{Deserialize, Serialize};

/// An exact provider/model pair allowed in a review-flow model group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewModelRef {
    pub provider_id: String,
    pub model_id: String,
}

/// Review-flow model pools. List order is preserved for assignment rotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewModelGroups {
    #[serde(default = "default_fast_models")]
    pub fast: Vec<ReviewModelRef>,
    #[serde(default = "default_standard_models")]
    pub standard: Vec<ReviewModelRef>,
    #[serde(default = "default_advanced_models")]
    pub advanced: Vec<ReviewModelRef>,
}

impl Default for ReviewModelGroups {
    fn default() -> Self {
        Self {
            fast: default_fast_models(),
            standard: default_standard_models(),
            advanced: default_advanced_models(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewModelTier {
    Fast,
    Standard,
    Advanced,
}

impl ReviewModelGroups {
    pub fn models(&self, tier: ReviewModelTier) -> &[ReviewModelRef] {
        match tier {
            ReviewModelTier::Fast => &self.fast,
            ReviewModelTier::Standard => &self.standard,
            ReviewModelTier::Advanced => &self.advanced,
        }
    }
}

fn pair(provider_id: &str, model_id: &str) -> ReviewModelRef {
    ReviewModelRef {
        provider_id: provider_id.to_owned(),
        model_id: model_id.to_owned(),
    }
}

fn default_fast_models() -> Vec<ReviewModelRef> {
    vec![pair("claude", "haiku-5.5"), pair("codex", "gpt-6-luna")]
}

fn default_standard_models() -> Vec<ReviewModelRef> {
    vec![pair("claude", "sonnet-5.5"), pair("codex", "gpt-6-sol")]
}

fn default_advanced_models() -> Vec<ReviewModelRef> {
    vec![pair("claude", "opus-5.5"), pair("codex", "gpt-6-astra")]
}
