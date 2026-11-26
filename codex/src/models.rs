use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, Reasoning, SupportedEffort, SupportedParams,
};
use serde::Deserialize;

const PROVIDER_ID: &str = "codex";

#[derive(Default, Deserialize)]
pub(super) struct CodexModelsResponse {
    #[serde(default)]
    pub(super) models: Vec<CodexModel>,
}

#[derive(Default, Deserialize)]
pub(super) struct CodexModel {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    default_reasoning_level: Option<CodexEffort>,
    #[serde(default)]
    supported_reasoning_levels: Vec<CodexReasoningLevel>,
    #[serde(default)]
    visibility: CodexVisibility,
    #[serde(default)]
    supported_in_api: bool,
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    input_modalities: Vec<CodexModality>,
}

#[derive(Default, Deserialize)]
struct CodexReasoningLevel {
    effort: CodexEffort,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CodexVisibility {
    List,
    #[default]
    #[serde(other)]
    Hidden,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CodexEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    #[default]
    #[serde(other)]
    Unsupported,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CodexModality {
    Text,
    Image,
    Audio,
    Video,
    #[serde(alias = "pdf")]
    File,
    #[default]
    #[serde(other)]
    Unsupported,
}

impl CodexModel {
    pub(super) fn into_model(self) -> Option<Model> {
        if self.slug.trim().is_empty()
            || !self.supported_in_api
            || !matches!(self.visibility, CodexVisibility::List)
        {
            return None;
        }
        let supported_efforts = self
            .supported_reasoning_levels
            .into_iter()
            .filter_map(|level| level.effort.into_supported_effort())
            .collect::<Vec<_>>();
        let default_effort = self
            .default_reasoning_level
            .and_then(CodexEffort::into_supported_effort);
        let input = self
            .input_modalities
            .into_iter()
            .filter_map(CodexModality::into_modality)
            .collect::<Vec<_>>();
        let id = self.slug;
        let has_reasoning = !supported_efforts.is_empty() || default_effort.is_some();
        Some(Model {
            canonical_slug: format!("{PROVIDER_ID}/{id}"),
            name: self.display_name.unwrap_or_else(|| id.clone()),
            description: self.description,
            modalities: Modalities {
                input: if input.is_empty() {
                    vec![Modality::Text]
                } else {
                    input
                },
                output: vec![Modality::Text],
            },
            reasoning: Reasoning {
                default_enabled: has_reasoning,
                default_effort,
                supported_efforts,
                ..Reasoning::default()
            },
            limit: Limit {
                context: self.context_window,
                output: None,
            },
            supported_params: supported_params(has_reasoning),
            id,
            provider_id: PROVIDER_ID.to_owned(),
            ..Model::default()
        })
    }
}

impl CodexEffort {
    fn into_supported_effort(self) -> Option<SupportedEffort> {
        match self {
            Self::None => Some(SupportedEffort::None),
            Self::Minimal => Some(SupportedEffort::Minimal),
            Self::Low => Some(SupportedEffort::Low),
            Self::Medium => Some(SupportedEffort::Medium),
            Self::High => Some(SupportedEffort::High),
            Self::Xhigh => Some(SupportedEffort::Xhigh),
            Self::Max => Some(SupportedEffort::Max),
            Self::Unsupported => None,
        }
    }
}

impl CodexModality {
    fn into_modality(self) -> Option<Modality> {
        match self {
            Self::Text => Some(Modality::Text),
            Self::Image => Some(Modality::Image),
            Self::Audio => Some(Modality::Audio),
            Self::Video => Some(Modality::Video),
            Self::File => Some(Modality::File),
            Self::Unsupported => None,
        }
    }
}

fn supported_params(has_reasoning: bool) -> Vec<SupportedParams> {
    let mut params = vec![
        SupportedParams::Tools,
        SupportedParams::ToolChoice,
        SupportedParams::StructuredOutput,
    ];
    if has_reasoning {
        params.extend([SupportedParams::Reasoning, SupportedParams::ReasoningEffort]);
    }
    params
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
