//! `Anthropic` provider plugin for Norte Code.
//!
//! This plugin fetches the `Anthropic` model list at startup and exposes
//! it through the host's catalogue. It reads the key from the environment
//! (`ANTHROPIC_API_KEY`); if it is missing or the request fails the plugin
//! offers nothing and the person sees the ordinary missing-key error when
//! they pick a model.
//!
//! The catalogue refreshes every 15 minutes; it is a startup read, not a
//! per-request one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, Provider, Reasoning,
    SupportedParams, Wire, WireSchema,
};
use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const PROVIDER_ID: &str = "anthropic";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "anthropic-models.json";

#[derive(Default)]
struct AnthropicPlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for AnthropicPlugin {
    fn name(&self) -> &'static str {
        "anthropic"
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
    Box::into_raw(Box::new(AnthropicPlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let path = cache_path();
    let payload: Option<AnthropicModelsResponse> = path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .or_else(|| {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("norte-code-catalog-plugin-anthropic/0.1")
                .build()
                .and_then(|client| {
                    let mut request = client.get(MODELS_URL).header("anthropic-version", "2023-06-01");
                    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                        if !key.trim().is_empty() {
                            request = request.header("x-api-key", key);
                        }
                    }
                    request.send()
                })
                .ok()?;
            let payload = response.json::<AnthropicModelsResponse>().ok()?;
            if let Some(path) = path.as_deref() {
                write_cache(path, now, &payload);
            }
            Some(payload)
        });
    let Some(payload) = payload else {
        return Catalogue::default();
    };
    let models = payload.data.into_iter().map(model).collect::<Vec<_>>();
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
    data: Vec<AnthropicModel>,
}

#[derive(Clone, Serialize, Deserialize)]
struct AnthropicModel {
    id: String,
    display_name: Option<String>,
    #[serde(default)]
    max_input_tokens: Option<u64>,
    #[serde(default)]
    max_tokens: Option<u64>,
}

fn provider() -> Provider {
    Provider {
        name: "Anthropic".to_owned(),
        doc: Some("https://docs.anthropic.com/en/docs/build-with-claude/working-with-messages"
            .to_owned()),
        environment_keys: vec!["ANTHROPIC_API_KEY".to_owned()],
        wires: vec![Wire {
            api: "https://api.anthropic.com".to_owned(),
            schema: WireSchema::Anthropic,
            auth: None,
            headers: std::collections::BTreeMap::new(),
        }],
    }
}

fn model(source: AnthropicModel) -> Model {
    let id = source.id;
    Model {
        id: id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: format!("{PROVIDER_ID}/{id}"),
        name: source.display_name.unwrap_or(id),
        description: None,
        family: None,
        knowledge: None,
        release_date: None,
        last_updated: None,
        reasoning: Reasoning::default(),
        modalities: Modalities {
            input: vec![Modality::Text, Modality::Image],
            output: vec![Modality::Text],
        },
        open_weights: false,
        limit: Limit {
            context: source.max_input_tokens,
            output: source.max_tokens,
        },
        status: None,
        supported_params: vec![
            SupportedParams::Tools,
            SupportedParams::ToolChoice,
            SupportedParams::StructuredOutput,
            SupportedParams::Reasoning,
            SupportedParams::MaxTokens,
        ],
        pricing: None,
    }
}