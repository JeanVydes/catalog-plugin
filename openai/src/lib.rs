//! `OpenAI` provider plugin for Norte Code.
//!
//! This plugin fetches the `OpenAI` model list at startup and exposes
//! it through the host's catalogue. It reads `OPENAI_API_KEY` from the
//! environment and uses `Authorization: Bearer`. A missing or invalid key,
//! or any failure from the endpoint, yields an empty catalogue silently —
//! the person will get the ordinary missing-key error when they pick a
//! model, not a half-broken list.
//!
//! The catalogue refreshes every 15 minutes; it is a startup read, not a
//! per-request one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, Provider, Reasoning, SupportedParams, Wire,
    WireSchema,
};
use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://api.openai.com/v1/models";
const PROVIDER_ID: &str = "openai";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "openai-models.json";

#[derive(Default)]
struct OpenAiPlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for OpenAiPlugin {
    fn name(&self) -> &'static str {
        "openai"
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
    Box::into_raw(Box::new(OpenAiPlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let path = cache_path();
    let payload: Option<OpenAiModelsResponse> = path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .or_else(|| {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("norte-code-catalog-plugin-openai/0.1")
                .build()
                .and_then(|client| {
                    client
                        .get(MODELS_URL)
                        .header(
                            "Authorization",
                            format!("Bearer {}", std::env::var("OPENAI_API_KEY").unwrap_or_default()),
                        )
                        .send()
                })
                .ok()?;
            let payload = response.json::<OpenAiModelsResponse>().ok()?;
            if let Some(path) = path.as_deref() {
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
        .filter(|model| model.owned_by == "openai")
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

fn read_cache(path: &Path, now: u64) -> Option<OpenAiModelsResponse> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: CachedResponse = serde_json::from_str(&text).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.payload)
}

fn write_cache(path: &Path, fetched_at: u64, payload: &OpenAiModelsResponse) {
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
    payload: OpenAiModelsResponse,
}

#[derive(Clone, Serialize, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(Clone, Serialize, Deserialize)]
struct OpenAiModel {
    id: String,
    #[serde(default)]
    owned_by: String,
}

fn provider() -> Provider {
    Provider {
        name: "OpenAI".to_owned(),
        doc: Some("https://developers.openai.com/api/docs/models".to_owned()),
        environment_keys: vec!["OPENAI_API_KEY".to_owned()],
        wires: vec![Wire {
            api: "https://api.openai.com/v1".to_owned(),
            schema: WireSchema::OpenAI,
            auth: None,
            headers: BTreeMap::new(),
        }],
    }
}

fn model(source: OpenAiModel) -> Model {
    let id = source.id;
    Model {
        id: id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: format!("{PROVIDER_ID}/{id}"),
        name: id.clone(),
        description: None,
        family: None,
        knowledge: None,
        release_date: None,
        last_updated: None,
        reasoning: Reasoning {
            mandatory: false,
            default_enabled: true,
            default_effort: None,
            supported_efforts: Vec::new(),
            budget_supported: false,
        },
        modalities: Modalities {
            input: vec![Modality::Text, Modality::Image],
            output: vec![Modality::Text],
        },
        open_weights: false,
        limit: Limit::default(),
        status: None,
        supported_params: vec![
            SupportedParams::Tools,
            SupportedParams::ToolChoice,
            SupportedParams::StructuredOutput,
            SupportedParams::Reasoning,
        ],
        pricing: None,
    }
}