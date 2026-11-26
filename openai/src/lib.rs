//! `OpenAI` provider plugin for Norte Code.

use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, ModelStatus, Provider, Reasoning, Wire, WireSchema,
};
use norte_types::SupportedParam;
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://api.openai.com/v1/models";
const PROVIDER_ID: &str = "openai";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "openai-models.json";

#[derive(Default)]
struct OpenAiPlugin {
    state: OnceLock<PluginState>,
}

impl OpenAiPlugin {
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

impl Plugin for OpenAiPlugin {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn catalogue(&self) -> Catalogue {
        self.state().catalogue.clone()
    }

    fn widgets(&self) -> Vec<norte_code_plugin_host::WidgetDefinition> {
        vec![norte_code_plugin_host::WidgetDefinition {
            id: norte_code_wire::plugin_widgets::WidgetId::new("catalog-status"),
            title: "Catalogue status".to_owned(),
        }]
    }

    fn widget(
        &self,
        id: &norte_code_wire::plugin_widgets::WidgetId,
        invocation: &norte_code_plugin_host::PluginInvocation,
    ) -> Option<norte_code_wire::plugin_widgets::WidgetView> {
        use norte_code_wire::plugin_widgets::{Widget, WidgetView};

        let selection = &invocation.selection;
        if id.as_str() != "catalog-status" || selection.provider.as_deref() != Some(self.name()) {
            return None;
        }
        let text = self.state().status.clone()?;
        Some(WidgetView {
            title: Some(self.name().to_uppercase()),
            content: vec![Widget::Text { text }],
        })
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
    Box::into_raw(Box::new(OpenAiPlugin::default()))
}

fn fetch_catalogue() -> Result<Catalogue, CatalogError> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or(CatalogError::MissingApiKey)?;
    let now = unix_now();
    let cache_path = cache_path(&api_key);
    let payload = cache_path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .map_or_else(|| fetch_models(&api_key, cache_path.as_deref(), now), Ok)?;

    let models = payload
        .data
        .into_iter()
        .filter(is_chat_model)
        .map(model)
        .map(|model| (model.id.clone(), model))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    Ok(Catalogue {
        providers: BTreeMap::from([(PROVIDER_ID.to_owned(), provider())]),
        models,
    })
}

fn fetch_models(
    api_key: &str,
    cache_path: Option<&Path>,
    now: u64,
) -> Result<OpenAiModelsResponse, CatalogError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("norte-code-catalog-plugin-openai/0.1")
        .build()?;
    let payload = client
        .get(MODELS_URL)
        .bearer_auth(api_key)
        .send()?
        .error_for_status()?
        .json::<OpenAiModelsResponse>()?;
    if let Some(path) = cache_path {
        write_cache(path, now, &payload);
    }
    Ok(payload)
}

fn is_chat_model(source: &OpenAiModel) -> bool {
    let id = source.id.to_ascii_lowercase();
    ![
        "embedding",
        "davinci-",
        "babbage-",
        "instruct",
        "code-",
        "whisper",
        "tts-",
        "-tts",
        "transcribe",
        "audio",
        "realtime",
        "deep-research",
        "computer-use",
        "dall-e",
        "gpt-image",
        "moderation",
        "sora",
    ]
    .iter()
    .any(|unsupported_family| id.contains(unsupported_family))
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
    owned_by: Option<String>,
    #[serde(default)]
    shutdown_date: Option<String>,
}

fn provider() -> Provider {
    Provider {
        name: "OpenAI".to_owned(),
        doc: Some("https://developers.openai.com/api/docs/models".to_owned()),
        environment_keys: vec!["OPENAI_API_KEY".to_owned()],
        wires: vec![Wire {
            quirks: std::collections::BTreeSet::new(),
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
        supported_params_complete: false,
        quirks: BTreeMap::new(),
        id: id.clone(),
        provider_id: PROVIDER_ID.to_owned(),
        canonical_slug: format!("{PROVIDER_ID}/{id}"),
        name: id,
        description: None,
        family: None,
        reasoning: Reasoning::default(),
        knowledge: None,
        release_date: None,
        last_updated: None,
        modalities: Modalities {
            input: vec![Modality::Text],
            output: vec![Modality::Text],
        },
        open_weights: false,
        limit: Limit::default(),
        status: source.shutdown_date.map(|_| ModelStatus::Deprecated),
        supported_params: vec![
            SupportedParam::Tools,
            SupportedParam::ToolChoice,
            SupportedParam::MaxOutputTokens,
        ],
        pricing: None,
    }
}

#[derive(Debug)]
enum CatalogError {
    MissingApiKey,
    Request(reqwest::Error),
}

impl From<reqwest::Error> for CatalogError {
    fn from(error: reqwest::Error) -> Self {
        Self::Request(error)
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => formatter.write_str("OPENAI_API_KEY is not set"),
            Self::Request(error) => write!(formatter, "OpenAI model listing failed: {error}"),
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
