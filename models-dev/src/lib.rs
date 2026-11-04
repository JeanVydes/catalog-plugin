//! `models.dev` provider and model catalogue plugin for Norte Code.
//!
//! The source contains providers that use different SDK packages. This plugin
//! keeps only the four packages Norte Code can speak, and gives every retained
//! provider a unique `models-dev/` namespace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_protocol::catalog::{
    Limit, Modalities, Modality, Model, Pricing, Provider, Reasoning, SupportedParams, Wire,
    WireSchema,
};
use norte_code_protocol::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::{Deserialize, Serialize};

const CATALOG_URL: &str = "https://models.dev/catalog.json?type=all";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "models-dev-catalog.json";

/// SDK packages supported by Norte Code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum SupportedNpm {
    #[serde(rename = "@ai-sdk/openai")]
    OpenAi,
    #[serde(rename = "@ai-sdk/openai-compatible")]
    OpenAiCompatible,
    #[serde(rename = "@ai-sdk/anthropic")]
    Anthropic,
    #[serde(rename = "@ai-sdk/google")]
    Google,
}

impl SupportedNpm {
    fn schema(self) -> WireSchema {
        match self {
            Self::OpenAi | Self::OpenAiCompatible => WireSchema::OpenAI,
            Self::Anthropic => WireSchema::Anthropic,
            Self::Google => WireSchema::Google,
        }
    }

    fn default_api(self) -> &'static str {
        match self {
            Self::OpenAi | Self::OpenAiCompatible => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Google => "https://generativelanguage.googleapis.com/v1beta",
        }
    }
}

/// A plugin that imports the filtered `models.dev` catalogue.
#[derive(Default)]
pub struct ModelsDevPlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for ModelsDevPlugin {
    fn name(&self) -> &'static str {
        "models-dev"
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
    Box::into_raw(Box::new(ModelsDevPlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let path = cache_path();
    let payload = path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .or_else(|| {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("norte-code-catalog-plugin-models-dev/0.1")
                .build()
                .and_then(|client| client.get(CATALOG_URL).send())
                .and_then(reqwest::blocking::Response::error_for_status)
                .ok()?;
            let payload = response.json::<ModelsDevCatalog>().ok()?;
            if let Some(path) = path.as_deref() {
                write_cache(path, now, &payload);
            }
            Some(payload)
        });
    payload.map_or_else(Catalogue::default, catalogue)
}

fn catalogue(source: ModelsDevCatalog) -> Catalogue {
    let mut providers = BTreeMap::new();
    let mut models = Vec::new();

    for (provider_id, source_provider) in source.providers {
        let Some(npm) = source_provider.npm.as_deref().and_then(parse_npm) else {
            continue;
        };
        let id = format!("models-dev/{provider_id}");
        let api = source_provider
            .api
            .unwrap_or_else(|| npm.default_api().to_owned());
        providers.insert(
            id.clone(),
            Provider {
                name: format!("Model.dev - {}", source_provider.name),
                doc: Some("https://models.dev".to_owned()),
                environment_keys: source_provider.env,
                wires: vec![Wire {
                    api,
                    schema: npm.schema(),
                    auth: None,
                }],
            },
        );
        for (model_id, source_model) in source_provider.models {
            models.push(model(source_model, model_id, &id));
        }
    }
    Catalogue { providers, models }
}

fn model(source: ModelsDevModel, model_id: String, provider_id: &str) -> Model {
    let id = source.id.unwrap_or(model_id);
    Model {
        id: id.clone(),
        provider_id: provider_id.to_owned(),
        canonical_slug: format!("{provider_id}/{id}"),
        name: source.name,
        description: source.description,
        family: source.family,
        reasoning: reasoning(source.reasoning),
        knowledge: None,
        release_date: source.release_date,
        last_updated: source.last_updated,
        modalities: modalities(source.modalities),
        open_weights: source.open_weights,
        limit: Limit {
            context: source.limit.as_ref().and_then(|limit| limit.context),
            output: source.limit.as_ref().and_then(|limit| limit.output),
        },
        status: None,
        supported_params: source
            .supported_params
            .into_iter()
            .filter_map(|value| supported_param(&value))
            .collect(),
        pricing: source.pricing.as_ref().map(pricing),
    }
}

fn reasoning(enabled: bool) -> Reasoning {
    Reasoning {
        default_enabled: enabled,
        ..Reasoning::default()
    }
}

fn modalities(values: Option<ModelsDevModalities>) -> Modalities {
    let Some(values) = values else {
        return Modalities {
            input: vec![Modality::Text],
            output: vec![Modality::Text],
        };
    };
    Modalities {
        input: values
            .input
            .iter()
            .filter_map(|value| modality(value))
            .collect(),
        output: values
            .output
            .iter()
            .filter_map(|value| modality(value))
            .collect(),
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

fn pricing(value: &ModelsDevPricing) -> Pricing {
    Pricing {
        input: value.input,
        output: value.output,
        cached: value.cache_read,
    }
}

fn supported_param(value: &str) -> Option<SupportedParams> {
    match value {
        "frequency_penalty" => Some(SupportedParams::FrequencyPenalty),
        "max_tokens" => Some(SupportedParams::MaxTokens),
        "reasoning" => Some(SupportedParams::Reasoning),
        "reasoning_effort" => Some(SupportedParams::ReasoningEffort),
        "response_format" => Some(SupportedParams::ResponseFormat),
        "structured_outputs" => Some(SupportedParams::StructuredOutput),
        "temperature" => Some(SupportedParams::Temperature),
        "tool_choice" => Some(SupportedParams::ToolChoice),
        "tools" => Some(SupportedParams::Tools),
        "top_p" => Some(SupportedParams::TopP),
        _ => None,
    }
}

fn parse_npm(value: &str) -> Option<SupportedNpm> {
    match value {
        "@ai-sdk/openai" => Some(SupportedNpm::OpenAi),
        "@ai-sdk/openai-compatible" => Some(SupportedNpm::OpenAiCompatible),
        "@ai-sdk/anthropic" => Some(SupportedNpm::Anthropic),
        "@ai-sdk/google" => Some(SupportedNpm::Google),
        _ => None,
    }
}

fn cache_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|path| path.join("norte-code").join("plugins").join(CACHE_FILE))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn read_cache(path: &Path, now: u64) -> Option<ModelsDevCatalog> {
    let cache: CachedCatalog = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.catalog)
}

fn write_cache(path: &Path, fetched_at: u64, catalog: &ModelsDevCatalog) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let cache = CachedCatalog {
        fetched_at,
        catalog: catalog.clone(),
    };
    if let Ok(text) = serde_json::to_string(&cache) {
        let _ = std::fs::write(path, text);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelsDevCatalog {
    providers: BTreeMap<String, ModelsDevProvider>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedCatalog {
    fetched_at: u64,
    catalog: ModelsDevCatalog,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelsDevProvider {
    name: String,
    #[serde(default)]
    env: Vec<String>,
    npm: Option<String>,
    api: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, ModelsDevModel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelsDevModel {
    id: Option<String>,
    name: String,
    description: Option<String>,
    family: Option<String>,
    reasoning: bool,
    #[serde(default)]
    modalities: Option<ModelsDevModalities>,
    open_weights: bool,
    limit: Option<ModelsDevLimit>,
    #[serde(default)]
    supported_params: Vec<String>,
    pricing: Option<ModelsDevPricing>,
    release_date: Option<String>,
    last_updated: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelsDevModalities {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelsDevLimit {
    context: Option<u64>,
    output: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelsDevPricing {
    input: Option<f64>,
    output: Option<f64>,
    #[serde(alias = "cache_read")]
    cache_read: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_namespaces_providers() {
        let source: ModelsDevCatalog = serde_json::from_value(serde_json::json!({
            "providers": {
                "deepinfra": {
                    "name": "Deep Infra", "env": ["DEEPINFRA_API_KEY"],
                    "npm": "@ai-sdk/deepinfra", "api": null, "models": {}
                },
                "acme": {
                    "name": "Acme", "env": ["ACME_KEY"],
                    "npm": "@ai-sdk/openai-compatible", "api": "https://acme/v1",
                    "models": {"m": {
                        "id": "m", "name": "Model", "reasoning": false,
                        "open_weights": true, "limit": {"context": 1000, "output": 100}
                    }}
                }
            }
        }))
        .expect("fixture");
        let result = catalogue(source);
        assert!(!result.providers.contains_key("models-dev/deepinfra"));
        let provider = result.providers.get("models-dev/acme").expect("provider");
        assert_eq!(provider.name, "Model.dev - Acme");
        assert_eq!(provider.environment_keys, ["ACME_KEY"]);
        assert_eq!(provider.wires[0].schema, WireSchema::OpenAI);
        assert_eq!(result.models[0].provider_id, "models-dev/acme");
    }

    #[test]
    fn cache_expires_after_fifteen_minutes() {
        let path = std::env::temp_dir().join(format!("models-dev-cache-{}", std::process::id()));
        let catalog = ModelsDevCatalog {
            providers: BTreeMap::new(),
        };
        write_cache(&path, 100, &catalog);
        assert!(read_cache(&path, 100 + CACHE_TTL.as_secs() - 1).is_some());
        assert!(read_cache(&path, 100 + CACHE_TTL.as_secs()).is_none());
        std::fs::remove_file(path).expect("remove test cache");
    }
}
