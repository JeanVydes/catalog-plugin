//! Google Gemini provider plugin for Norte Code.

use std::collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, Provider, Reasoning, SupportedParams, Wire, WireSchema,
};
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const PROVIDER_ID: &str = "google";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "google-models.json";
const PAGE_SIZE: &str = "1000";
const MAX_PAGES: usize = 100;

#[derive(Default)]
struct GooglePlugin {
    state: OnceLock<PluginState>,
}

impl GooglePlugin {
    fn state(&self) -> &PluginState {
        self.state.get_or_init(|| match fetch_catalogue() {
            Ok(catalogue) => PluginState {
                catalogue,
                status: None,
            },
            Err(error) => PluginState {
                catalogue: Catalogue::default(),
                status: Some(error.to_string()),
            },
        })
    }
}

impl Plugin for GooglePlugin {
    fn name(&self) -> &'static str {
        "google"
    }

    fn catalogue(&self) -> Catalogue {
        self.state().catalogue.clone()
    }

    fn status(&self) -> Option<String> {
        self.state().status.clone()
    }
}

struct PluginState {
    catalogue: Catalogue,
    status: Option<String>,
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

fn fetch_catalogue() -> Result<Catalogue, CatalogError> {
    let api_key = std::env::var("GOOGLE_API_KEY")
        .or_else(|_| std::env::var("GEMINI_API_KEY"))
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or(CatalogError::MissingApiKey)?;
    let now = unix_now();
    let cache_path = cache_path(&api_key);
    let payload = cache_path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .map_or_else(|| fetch_models(&api_key, cache_path.as_deref(), now), Ok)?;

    let mut models = BTreeMap::new();
    for source in payload.models.into_iter().filter(is_generate_content_model) {
        let model = model(source);
        models.entry(model.id.clone()).or_insert(model);
    }
    Ok(Catalogue {
        providers: BTreeMap::from([(PROVIDER_ID.to_owned(), provider())]),
        models: models.into_values().collect(),
    })
}

fn fetch_models(
    api_key: &str,
    cache_path: Option<&Path>,
    now: u64,
) -> Result<GoogleModelsResponse, CatalogError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("norte-code-catalog-plugin-google/0.1")
        .build()?;
    let mut models = Vec::new();
    let mut page_token: Option<String> = None;
    let mut seen_tokens = BTreeSet::new();

    for _ in 0..MAX_PAGES {
        let mut request = client
            .get(MODELS_URL)
            .header("x-goog-api-key", api_key)
            .query(&[("pageSize", PAGE_SIZE)]);
        if let Some(token) = page_token.as_deref() {
            request = request.query(&[("pageToken", token)]);
        }
        let page = request
            .send()?
            .error_for_status()?
            .json::<GoogleModelsResponse>()?;
        models.extend(page.models);
        let Some(next_token) = page
            .next_page_token
            .filter(|token| !token.trim().is_empty())
        else {
            let payload = GoogleModelsResponse {
                models,
                next_page_token: None,
            };
            if let Some(path) = cache_path {
                write_cache(path, now, &payload);
            }
            return Ok(payload);
        };
        if !seen_tokens.insert(next_token.clone()) {
            return Err(CatalogError::PaginationLoop);
        }
        page_token = Some(next_token);
    }
    Err(CatalogError::TooManyPages)
}

fn is_generate_content_model(source: &GoogleModel) -> bool {
    source
        .supported_generation_methods
        .iter()
        .any(|method| method == "generateContent")
}

fn cache_path(api_key: &str) -> Option<PathBuf> {
    let cache_root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let mut hasher = DefaultHasher::new();
    api_key.hash(&mut hasher);
    Some(cache_root.join("norte-code").join("plugins").join(format!(
        "{}-{:016x}",
        CACHE_FILE,
        hasher.finish()
    )))
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
#[serde(rename_all = "camelCase")]
struct GoogleModelsResponse {
    #[serde(default)]
    models: Vec<GoogleModel>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleModel {
    name: String,
    #[serde(default)]
    base_model_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_token_limit: Option<u64>,
    #[serde(default)]
    output_token_limit: Option<u64>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
    #[serde(default)]
    thinking: bool,
}

fn provider() -> Provider {
    Provider {
        name: "Google Gemini".to_owned(),
        doc: Some("https://ai.google.dev/api/models".to_owned()),
        environment_keys: vec!["GOOGLE_API_KEY".to_owned(), "GEMINI_API_KEY".to_owned()],
        wires: vec![Wire {
            api: "https://generativelanguage.googleapis.com/v1beta".to_owned(),
            schema: WireSchema::Google,
            auth: None,
            headers: BTreeMap::new(),
        }],
    }
}

fn model(source: GoogleModel) -> Model {
    let id = source
        .base_model_id
        .unwrap_or_else(|| model_id(&source.name));
    let reasoning = if source.thinking {
        Reasoning {
            mandatory: false,
            default_enabled: false,
            default_effort: None,
            supported_efforts: Vec::new(),
            budget_supported: true,
        }
    } else {
        Reasoning::default()
    };
    let mut supported_params = vec![SupportedParams::MaxTokens];
    if id.starts_with("gemini-") {
        supported_params.extend([
            SupportedParams::Tools,
            SupportedParams::ToolChoice,
            SupportedParams::StructuredOutput,
        ]);
    }
    if source.thinking {
        supported_params.push(SupportedParams::Reasoning);
    }
    Model {
        id: id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: format!("{PROVIDER_ID}/{id}"),
        name: source.display_name.unwrap_or_else(|| id.clone()),
        description: source.description,
        family: None,
        reasoning,
        knowledge: None,
        release_date: None,
        last_updated: None,
        modalities: Modalities {
            input: vec![Modality::Text],
            output: vec![Modality::Text],
        },
        open_weights: false,
        limit: Limit {
            context: source.input_token_limit,
            output: source.output_token_limit,
        },
        status: None,
        supported_params,
        pricing: None,
    }
}

fn model_id(name: &str) -> String {
    name.strip_prefix("models/").unwrap_or(name).to_owned()
}

#[derive(Debug)]
enum CatalogError {
    MissingApiKey,
    Request(reqwest::Error),
    PaginationLoop,
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
            Self::MissingApiKey => {
                formatter.write_str("GOOGLE_API_KEY or GEMINI_API_KEY is not set")
            }
            Self::Request(error) => write!(formatter, "Google model listing failed: {error}"),
            Self::PaginationLoop => {
                formatter.write_str("Google model listing returned a repeated page token")
            }
            Self::TooManyPages => {
                formatter.write_str("Google model listing exceeded the page limit")
            }
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
