//! Anthropic's dynamic model catalogue and Messages API provider mapping.

use std::collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_plugin_host::plugins::Catalogue;
use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, ModelStatus, Provider, Reasoning, SupportedEffort, Wire,
    WireSchema,
};
use norte_types::SupportedParam;
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const PROVIDER_ID: &str = "anthropic";
const PROVIDER_NAME: &str = "Anthropic";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "anthropic-models.json";
const API_VERSION: &str = "2023-06-01";
const PAGE_SIZE: &str = "1000";
const MAX_PAGES: usize = 100;

/// Fetches Anthropic models and exposes them through this plugin's provider.
///
/// # Errors
///
/// Returns an error when the API key is absent, the HTTP request fails, or
/// pagination cannot complete safely.
pub(super) fn fetch_catalogue() -> Result<Catalogue, CatalogError> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or(CatalogError::MissingApiKey)?;
    let workspace_id = workspace_id();
    let now = unix_now();
    let cache_path = cache_path(&api_key, workspace_id.as_deref());
    let payload = cache_path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .map_or_else(
            || {
                fetch_models(
                    &api_key,
                    workspace_id.as_deref(),
                    cache_path.as_deref(),
                    now,
                )
            },
            Ok,
        )?;

    let models = payload
        .data
        .into_iter()
        .filter(|source| source.lifecycle != ModelLifecycle::Retired)
        .map(model)
        .map(|model| (model.id.clone(), model))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    Ok(Catalogue {
        providers: BTreeMap::from([(PROVIDER_ID.to_owned(), provider(workspace_id.as_deref()))]),
        models,
    })
}

fn fetch_models(
    api_key: &str,
    workspace_id: Option<&str>,
    cache_path: Option<&Path>,
    now: u64,
) -> Result<AnthropicModelsResponse, CatalogError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("norte-code-catalog-plugin-anthropic/0.1")
        .build()?;
    let mut data = Vec::new();
    let mut after_id: Option<String> = None;
    let mut seen_cursors = BTreeSet::new();

    for _ in 0..MAX_PAGES {
        let mut request = client
            .get(MODELS_URL)
            .header("anthropic-version", API_VERSION)
            .header("x-api-key", api_key)
            .query(&[("limit", PAGE_SIZE)]);
        if let Some(workspace_id) = workspace_id {
            request = request.header("anthropic-workspace-id", workspace_id);
        }
        if let Some(cursor) = after_id.as_deref() {
            request = request.query(&[("after_id", cursor)]);
        }
        let page = request
            .send()?
            .error_for_status()?
            .json::<AnthropicModelsResponse>()?;
        data.extend(page.data);
        if !page.has_more {
            let payload = AnthropicModelsResponse {
                data,
                has_more: false,
                first_id: None,
                last_id: None,
            };
            if let Some(path) = cache_path {
                write_cache(path, now, &payload);
            }
            return Ok(payload);
        }
        let Some(cursor) = page.last_id.filter(|cursor| !cursor.trim().is_empty()) else {
            return Err(CatalogError::MissingPaginationCursor);
        };
        if !seen_cursors.insert(cursor.clone()) {
            return Err(CatalogError::PaginationLoop);
        }
        after_id = Some(cursor);
    }
    Err(CatalogError::TooManyPages)
}

fn cache_path(api_key: &str, workspace_id: Option<&str>) -> Option<PathBuf> {
    let cache_root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let mut hasher = DefaultHasher::new();
    api_key.hash(&mut hasher);
    workspace_id.hash(&mut hasher);
    Some(cache_root.join("norte-code").join("plugins").join(format!(
        "{}-{:016x}",
        CACHE_FILE,
        hasher.finish()
    )))
}

fn workspace_id() -> Option<String> {
    std::env::var("ANTHROPIC_WORKSPACE_ID")
        .ok()
        .map(|workspace_id| workspace_id.trim().to_owned())
        .filter(|workspace_id| !workspace_id.is_empty())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn read_cache(path: &Path, now: u64) -> Option<AnthropicModelsResponse> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: CachedResponse = serde_json::from_str(&text).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.payload)
}

fn write_cache(path: &Path, fetched_at: u64, payload: &AnthropicModelsResponse) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let cache = CachedResponse {
        fetched_at,
        payload: payload.clone(),
    };
    let Ok(text) = serde_json::to_string(&cache) else {
        return;
    };
    let _ = std::fs::write(path, text);
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedResponse {
    fetched_at: u64,
    payload: AnthropicModelsResponse,
}

#[derive(Clone, Serialize, Deserialize)]
struct AnthropicModelsResponse {
    #[serde(default)]
    data: Vec<AnthropicModel>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    first_id: Option<String>,
    #[serde(default)]
    last_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct AnthropicModel {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    max_input_tokens: Option<u64>,
    #[serde(default)]
    max_tokens: Option<u64>,
    #[serde(default)]
    capabilities: Option<ModelCapabilities>,
    #[serde(default)]
    lifecycle: ModelLifecycle,
    #[serde(default)]
    line: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelLifecycle {
    #[default]
    Active,
    Deprecated,
    Retired,
    #[serde(other)]
    Other,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct ModelCapabilities {
    #[serde(default)]
    image_input: Option<CapabilitySupport>,
    #[serde(default)]
    pdf_input: Option<CapabilitySupport>,
    #[serde(default)]
    structured_outputs: Option<CapabilitySupport>,
    #[serde(default)]
    thinking: Option<ThinkingCapability>,
    #[serde(default)]
    effort: Option<EffortCapability>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct CapabilitySupport {
    #[serde(default)]
    supported: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct ThinkingCapability {
    #[serde(default)]
    supported: bool,
    #[serde(default)]
    types: Option<ThinkingTypes>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct ThinkingTypes {
    #[serde(default)]
    adaptive: Option<CapabilitySupport>,
    #[serde(default)]
    disabled: Option<CapabilitySupport>,
    #[serde(default)]
    enabled: Option<CapabilitySupport>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct EffortCapability {
    #[serde(default)]
    supported: bool,
    #[serde(default)]
    low: Option<CapabilitySupport>,
    #[serde(default)]
    medium: Option<CapabilitySupport>,
    #[serde(default)]
    high: Option<CapabilitySupport>,
    #[serde(default)]
    xhigh: Option<CapabilitySupport>,
    #[serde(default)]
    max: Option<CapabilitySupport>,
}

fn provider(workspace_id: Option<&str>) -> Provider {
    let headers = workspace_id.map_or_else(BTreeMap::new, |workspace_id| {
        BTreeMap::from([("anthropic-workspace-id".to_owned(), workspace_id.to_owned())])
    });
    Provider {
        name: PROVIDER_NAME.to_owned(),
        doc: Some("https://docs.anthropic.com/en/api/models-list".to_owned()),
        environment_keys: vec!["ANTHROPIC_API_KEY".to_owned()],
        wires: vec![Wire {
            quirks: std::collections::BTreeSet::new(),
            api: "https://api.anthropic.com".to_owned(),
            schema: WireSchema::Anthropic,
            auth: None,
            headers,
        }],
    }
}

fn model(source: AnthropicModel) -> Model {
    let capabilities = source.capabilities.unwrap_or_default();
    let mut input = vec![Modality::Text];
    if supported(capabilities.image_input.as_ref()) {
        input.push(Modality::Image);
    }
    if supported(capabilities.pdf_input.as_ref()) {
        input.push(Modality::File);
    }

    let thinking = capabilities.thinking.unwrap_or_default();
    let effort = capabilities.effort.unwrap_or_default();
    let supported_efforts = effort_list(&effort);
    let disabled_supported = thinking
        .types
        .as_ref()
        .is_some_and(|types| supported(types.disabled.as_ref()));
    let enabled_supported = thinking
        .types
        .as_ref()
        .is_some_and(|types| supported(types.enabled.as_ref()));
    let adaptive_supported = thinking
        .types
        .as_ref()
        .is_some_and(|types| supported(types.adaptive.as_ref()));
    let mut supported_params = vec![SupportedParam::Tools, SupportedParam::ToolChoice];
    if supported(capabilities.structured_outputs.as_ref()) {
        supported_params.push(SupportedParam::StructuredOutput);
    }
    if thinking.supported {
        supported_params.push(SupportedParam::Reasoning);
    }
    if effort.supported {
        supported_params.push(SupportedParam::ReasoningEffort);
    }

    let id = source.id;
    Model {
        supported_params_complete: false,
        quirks: BTreeMap::new(),
        id: id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: format!("{PROVIDER_ID}/{id}"),
        name: source.display_name.unwrap_or_else(|| id.clone()),
        description: None,
        family: source.line,
        reasoning: Reasoning {
            mandatory: thinking.supported
                && !disabled_supported
                && (enabled_supported || adaptive_supported),
            default_enabled: false,
            default_effort: None,
            supported_efforts: if disabled_supported {
                [SupportedEffort::None]
                    .into_iter()
                    .chain(supported_efforts)
                    .collect()
            } else {
                supported_efforts
            },
            budget_supported: thinking.supported && enabled_supported,
        },
        knowledge: None,
        release_date: None,
        last_updated: None,
        modalities: Modalities {
            input,
            output: vec![Modality::Text],
        },
        open_weights: false,
        limit: Limit {
            context: nonzero(source.max_input_tokens),
            output: nonzero(source.max_tokens),
        },
        status: (source.lifecycle == ModelLifecycle::Deprecated).then_some(ModelStatus::Deprecated),
        supported_params,
        pricing: None,
    }
}

fn supported(capability: Option<&CapabilitySupport>) -> bool {
    capability.is_some_and(|capability| capability.supported)
}

fn effort_list(capability: &EffortCapability) -> Vec<SupportedEffort> {
    [
        (capability.low.as_ref(), SupportedEffort::Low),
        (capability.medium.as_ref(), SupportedEffort::Medium),
        (capability.high.as_ref(), SupportedEffort::High),
        (capability.xhigh.as_ref(), SupportedEffort::Xhigh),
        (capability.max.as_ref(), SupportedEffort::Max),
    ]
    .into_iter()
    .filter_map(|(support, effort)| {
        support
            .is_some_and(|support| support.supported)
            .then_some(effort)
    })
    .collect()
}

fn nonzero(value: Option<u64>) -> Option<u64> {
    value.filter(|value| *value > 0)
}

/// A failure while reading or decoding the Anthropic model catalogue.
#[derive(Debug)]
pub(super) enum CatalogError {
    /// `ANTHROPIC_API_KEY` is missing or empty.
    MissingApiKey,
    /// The HTTP request failed or returned an unsuccessful status.
    Request(reqwest::Error),
    /// The API said there are more results but gave no cursor.
    MissingPaginationCursor,
    /// The API repeated a pagination cursor.
    PaginationLoop,
    /// The API returned more pages than this plugin permits.
    TooManyPages,
}

impl From<reqwest::Error> for CatalogError {
    fn from(error: reqwest::Error) -> Self {
        Self::Request(error)
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => formatter.write_str("ANTHROPIC_API_KEY is not set"),
            Self::Request(error) => write!(formatter, "Anthropic model listing failed: {error}"),
            Self::MissingPaginationCursor => {
                formatter.write_str("Anthropic model listing has_more without last_id")
            }
            Self::PaginationLoop => {
                formatter.write_str("Anthropic model listing returned a repeated cursor")
            }
            Self::TooManyPages => {
                formatter.write_str("Anthropic model listing exceeded the page limit")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
