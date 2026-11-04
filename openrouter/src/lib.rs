//! `OpenRouter` provider plugin for Norte Code.
//!
//! The plugin reads `OpenRouter`'s public model catalogue at startup and exposes
//! the chat-compatible entries through the host's catalogue API. The API key
//! is not needed to list models; Norte Code obtains it later from its normal
//! provider secret handling when a request is sent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_protocol::catalog::{
    Limit, Modalities, Modality, Model, Pricing, Provider, Reasoning, SupportedEffort,
    SupportedParams, Wire, WireSchema,
};
use norte_code_protocol::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const PROVIDER_ID: &str = "openrouter";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "openrouter-models.json";

/// A plugin that adds `OpenRouter` and its currently published chat models.
#[derive(Default)]
pub struct OpenRouterPlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for OpenRouterPlugin {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    fn catalogue(&self) -> Catalogue {
        self.catalogue.get_or_init(fetch_catalogue).clone()
    }
}

/// Exports the plugin interface version expected by Norte Code.
#[unsafe(no_mangle)]
pub extern "C" fn norte_plugin_abi_version() -> u32 {
    PLUGIN_ABI_VERSION
}

/// Constructs the plugin for the host loader.
#[allow(improper_ctypes_definitions)]
#[unsafe(no_mangle)]
pub extern "C" fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(OpenRouterPlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let cache_path = cache_path();
    let cached = cache_path.as_deref().and_then(|path| read_cache(path, now));
    let payload = cached.clone().or_else(|| {
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("norte-code-catalog-plugin-openrouter/0.1")
            .build()
            .and_then(|client| client.get(MODELS_URL).send())
            .and_then(reqwest::blocking::Response::error_for_status);

        let Ok(response) = response else {
            return None;
        };
        let Ok(payload) = response.json::<OpenRouterResponse>() else {
            return None;
        };
        if let Some(path) = cache_path.as_deref() {
            write_cache(path, now, &payload);
        }
        Some(payload)
    });
    let Some(payload) = payload else {
        return Catalogue::default();
    };

    let models = payload
        .data
        .into_iter()
        .filter(has_chat_endpoint)
        .map(model)
        .collect();

    Catalogue {
        providers: BTreeMap::from([(PROVIDER_ID.to_owned(), provider())]),
        models,
    }
}

fn cache_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|cache| cache.join("norte-code").join("plugins").join(CACHE_FILE))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn read_cache(path: &Path, now: u64) -> Option<OpenRouterResponse> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: CachedResponse = serde_json::from_str(&text).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.payload)
}

fn write_cache(path: &Path, fetched_at: u64, payload: &OpenRouterResponse) {
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

fn provider() -> Provider {
    Provider {
        name: "OpenRouter".to_owned(),
        doc: Some("https://openrouter.ai/docs".to_owned()),
        environment_keys: vec!["OPENROUTER_API_KEY".to_owned()],
        wires: vec![Wire {
            api: "https://openrouter.ai/api/v1".to_owned(),
            schema: WireSchema::OpenAI,
            auth: None,
        }],
    }
}

fn has_chat_endpoint(model: &OpenRouterModel) -> bool {
    model
        .endpoints
        .iter()
        .any(|endpoint| endpoint == "/api/v1/chat/completions")
}

fn model(source: OpenRouterModel) -> Model {
    Model {
        id: source.id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: source.canonical_slug.unwrap_or_else(|| source.id.clone()),
        name: source.name,
        description: source.description,
        family: None,
        reasoning: source.reasoning.as_ref().map(reasoning).unwrap_or_default(),
        knowledge: source.knowledge_cutoff,
        release_date: None,
        last_updated: None,
        modalities: source
            .architecture
            .map_or_else(text_modalities, |architecture| modalities(&architecture)),
        open_weights: false,
        limit: Limit {
            context: source
                .context_length
                .or_else(|| source.top_provider.as_ref()?.context_length),
            output: source
                .top_provider
                .as_ref()
                .and_then(|provider| provider.max_completion_tokens),
        },
        status: None,
        supported_params: supported_params(&source.supported_parameters),
        pricing: source.pricing.map(pricing),
    }
}

fn modalities(source: &Architecture) -> Modalities {
    Modalities {
        input: source
            .input_modalities
            .iter()
            .filter_map(|value| modality(value))
            .collect(),
        output: source
            .output_modalities
            .iter()
            .filter_map(|value| modality(value))
            .collect(),
    }
}

fn text_modalities() -> Modalities {
    Modalities {
        input: vec![Modality::Text],
        output: vec![Modality::Text],
    }
}

fn modality(value: &str) -> Option<Modality> {
    match value {
        "text" => Some(Modality::Text),
        "image" => Some(Modality::Image),
        "audio" => Some(Modality::Audio),
        "video" => Some(Modality::Video),
        "file" | "pdf" => Some(Modality::File),
        _ => None,
    }
}

fn pricing(source: OpenRouterPricing) -> Pricing {
    Pricing {
        input: decimal(source.prompt),
        output: decimal(source.completion),
        cached: decimal(source.input_cache_read),
    }
}

fn decimal(value: Option<String>) -> Option<f64> {
    value.and_then(|value| value.parse().ok())
}

fn reasoning(source: &OpenRouterReasoning) -> Reasoning {
    Reasoning {
        mandatory: source.mandatory,
        default_enabled: source.default_enabled,
        default_effort: source.default_effort.as_deref().and_then(effort),
        supported_efforts: source
            .supported_efforts
            .iter()
            .filter_map(|value| effort(value))
            .collect(),
        budget_supported: false,
    }
}

fn effort(value: &str) -> Option<SupportedEffort> {
    match value {
        "none" => Some(SupportedEffort::None),
        "minimal" => Some(SupportedEffort::Minimal),
        "low" => Some(SupportedEffort::Low),
        "medium" => Some(SupportedEffort::Medium),
        "high" => Some(SupportedEffort::High),
        "xhigh" => Some(SupportedEffort::Xhigh),
        "max" => Some(SupportedEffort::Max),
        _ => None,
    }
}

fn supported_params(values: &[String]) -> Vec<SupportedParams> {
    let mut params: Vec<_> = values
        .iter()
        .filter_map(|value| supported_param(value))
        .collect();
    params.sort_unstable();
    params.dedup();
    params
}

fn supported_param(value: &str) -> Option<SupportedParams> {
    match value {
        "frequency_penalty" => Some(SupportedParams::FrequencyPenalty),
        "include_reasoning" => Some(SupportedParams::IncludeReasoning),
        "logprobs" => Some(SupportedParams::Logprobs),
        "max_tokens" => Some(SupportedParams::MaxTokens),
        "min_p" => Some(SupportedParams::MinP),
        "presence_penalty" => Some(SupportedParams::PresencePenalty),
        "reasoning" => Some(SupportedParams::Reasoning),
        "reasoning_effort" => Some(SupportedParams::ReasoningEffort),
        "response_format" => Some(SupportedParams::ResponseFormat),
        "seed" => Some(SupportedParams::Seed),
        "stop" => Some(SupportedParams::Stop),
        "structured_outputs" => Some(SupportedParams::StructuredOutput),
        "temperature" => Some(SupportedParams::Temperature),
        "tool_choice" => Some(SupportedParams::ToolChoice),
        "tools" => Some(SupportedParams::Tools),
        "top_p" => Some(SupportedParams::TopP),
        "top_logprobs" => Some(SupportedParams::TopLogprobs),
        "top_k" => Some(SupportedParams::TopK),
        "top_a" => Some(SupportedParams::TopA),
        "logit_bias" => Some(SupportedParams::LogitBias),
        "repetition_penalty" => Some(SupportedParams::RepetitionPenalty),
        "verbosity" => Some(SupportedParams::Verbosity),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OpenRouterResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OpenRouterModel {
    id: String,
    canonical_slug: Option<String>,
    name: String,
    description: Option<String>,
    context_length: Option<u64>,
    architecture: Option<Architecture>,
    pricing: Option<OpenRouterPricing>,
    top_provider: Option<TopProvider>,
    supported_parameters: Vec<String>,
    reasoning: Option<OpenRouterReasoning>,
    knowledge_cutoff: Option<String>,
    endpoints: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Architecture {
    input_modalities: Vec<String>,
    output_modalities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OpenRouterPricing {
    prompt: Option<String>,
    completion: Option<String>,
    input_cache_read: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TopProvider {
    context_length: Option<u64>,
    max_completion_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OpenRouterReasoning {
    #[serde(default)]
    mandatory: bool,
    #[serde(default)]
    default_enabled: bool,
    default_effort: Option<String>,
    #[serde(default)]
    supported_efforts: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedResponse {
    fetched_at: u64,
    payload: OpenRouterResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_openrouter_models_to_the_host_catalogue() {
        let source: OpenRouterModel = serde_json::from_value(serde_json::json!({
            "id": "qwen/qwen3",
            "canonical_slug": "qwen/qwen3-2025",
            "name": "Qwen: Qwen3",
            "description": "A model",
            "context_length": 131_072,
            "architecture": {
                "input_modalities": ["text", "image"],
                "output_modalities": ["text"]
            },
            "pricing": {
                "prompt": "0.000001",
                "completion": "0.000002",
                "input_cache_read": "0.0000001"
            },
            "top_provider": {
                "context_length": 131_072,
                "max_completion_tokens": 8192
            },
            "supported_parameters": ["tools", "temperature", "unknown"],
            "reasoning": {
                "mandatory": false,
                "default_enabled": true,
                "default_effort": "high",
                "supported_efforts": ["low", "high"]
            },
            "knowledge_cutoff": "2025-01-01",
            "endpoints": ["/api/v1/chat/completions"]
        }))
        .expect("fixture");

        let mapped = model(source);
        assert_eq!(mapped.provider_id, "openrouter");
        assert_eq!(mapped.limit.context, Some(131_072));
        assert_eq!(mapped.limit.output, Some(8_192));
        assert_eq!(mapped.modalities.input, [Modality::Text, Modality::Image]);
        assert_eq!(mapped.pricing.expect("pricing").input, Some(0.000_001));
        assert_eq!(mapped.reasoning.default_effort, Some(SupportedEffort::High));
        assert_eq!(
            mapped.supported_params,
            [SupportedParams::Temperature, SupportedParams::Tools]
        );
    }

    #[test]
    fn excludes_models_without_chat_completions() {
        let source: OpenRouterModel = serde_json::from_value(serde_json::json!({
            "id": "batch-only",
            "name": "Batch only",
            "supported_parameters": [],
            "endpoints": ["/api/v1/batches"]
        }))
        .expect("fixture");
        assert!(!has_chat_endpoint(&source));
    }

    #[test]
    fn uses_the_cache_for_fifteen_minutes() {
        let path = std::env::temp_dir().join(format!(
            "norte-code-openrouter-cache-{}",
            std::process::id()
        ));
        let payload = OpenRouterResponse { data: Vec::new() };
        write_cache(&path, 100, &payload);

        assert!(read_cache(&path, 100 + CACHE_TTL.as_secs() - 1).is_some());
        assert!(read_cache(&path, 100 + CACHE_TTL.as_secs()).is_none());

        std::fs::remove_file(path).expect("remove test cache");
    }
}
