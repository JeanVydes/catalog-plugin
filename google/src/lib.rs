//! `Google` Gemini provider plugin for Norte Code.
//!
//! Fetches the Gemini model list from `v1beta/models` at startup and exposes
//! it through the host's catalogue. The key is read from `GOOGLE_API_KEY`,
//! with `GEMINI_API_KEY` as fallback; both are valid for the Gemini API.
//! A missing or invalid key, or any failure from the endpoint, yields an
//! empty catalogue silently.
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

const MODELS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const PROVIDER_ID: &str = "google";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "google-models.json";

#[derive(Default)]
struct GooglePlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for GooglePlugin {
    fn name(&self) -> &'static str {
        "google"
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
    Box::into_raw(Box::new(GooglePlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let path = cache_path();
    let payload: Option<GoogleModelsResponse> = path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .or_else(|| {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("norte-code-catalog-plugin-google/0.1")
                .build()
                .and_then(|client| {
                    client
                        .get(MODELS_URL)
                        .header(
                            "x-goog-api-key",
                            std::env::var("GOOGLE_API_KEY")
                                .or_else(|_| std::env::var("GEMINI_API_KEY"))
                                .unwrap_or_default(),
                        )
                        .send()
                })
                .ok()?;
            let payload = response.json::<GoogleModelsResponse>().ok()?;
            if let Some(path) = path.as_deref() {
                write_cache(path, now, &payload);
            }
            Some(payload)
        });
    let Some(payload) = payload else {
        return Catalogue::default();
    };
    let models = payload
        .models
        .into_iter()
        .filter(|model| {
            model
                .supported_generation_methods
                .as_deref()
                .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
        })
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

fn read_cache(path: &Path, now: u64) -> Option<GoogleModelsResponse> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: CachedResponse = serde_json::from_str(&text).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.payload)
}

fn write_cache(path: &Path, fetched_at: u64, payload: &GoogleModelsResponse) {
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
    payload: GoogleModelsResponse,
}

#[derive(Clone, Serialize, Deserialize)]
struct GoogleModelsResponse {
    models: Vec<GoogleModel>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleModel {
    name: String,
    display_name: String,
    description: Option<String>,
    input_token_limit: Option<u64>,
    output_token_limit: Option<u64>,
    supported_generation_methods: Option<Vec<String>>,
}

fn provider() -> Provider {
    Provider {
        name: "Google Gemini".to_owned(),
        doc: Some(
            "https://ai.google.dev/gemini-api/docs/models/gemini".to_owned(),
        ),
        environment_keys: vec!["GOOGLE_API_KEY".to_owned(), "GEMINI_API_KEY".to_owned()],
        wires: vec![Wire {
            api: "https://generativelanguage.googleapis.com".to_owned(),
            schema: WireSchema::Google,
            auth: None,
            headers: BTreeMap::new(),
        }],
    }
}

fn model(source: GoogleModel) -> Model {
    let id = model_id(&source.name);
    Model {
        id: id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: format!("{PROVIDER_ID}/{id}"),
        name: source.display_name,
        description: source.description,
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
            input: vec![Modality::Text, Modality::Image, Modality::File],
            output: vec![Modality::Text],
        },
        open_weights: false,
        limit: Limit {
            context: source.input_token_limit,
            output: source.output_token_limit,
        },
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

fn model_id(name: &str) -> String {
    // The API returns "models/gemini-2.5-pro"
    name.strip_prefix("models/").unwrap_or(name).to_owned()
}