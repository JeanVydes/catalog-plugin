//! `Command Code` provider plugin for Norte Code.
//!
//! Command Code serves its catalogue at `/provider/v1/models`, which is read
//! at plugin load and cached for 15 minutes. Models are split across three
//! providers so each selection rides the wire its endpoint actually speaks:
//!
//!| Provider | Wire | Family |
//!| --- | --- | --- |
//!| `commandcode` | `openai` | `chat-completions` |
//!| `commandcode-responses` | `responses` | `responses` |
//!| `commandcode-anthropic` | `anthropic` | `messages` |
//!
//! The same model may appear in more than one provider when it is served on
//! multiple wires; that is the catalogue saying so, not a bug.
//!
//! The key is read from `CMD_API_KEY`, the same variable the CLI uses.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use norte_code_wire::catalog::{
    Limit, Modalities, Modality, Model, Provider, Reasoning, SupportedParams, Wire,
    WireSchema,
};
use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::{Deserialize, Serialize};

const MODELS_URL: &str = "https://api.commandcode.ai/provider/v1/models";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "commandcode-models.json";

#[derive(Default)]
struct CommandCodePlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for CommandCodePlugin {
    fn name(&self) -> &'static str {
        "commandcode"
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
    Box::into_raw(Box::new(CommandCodePlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let path = cache_path();
    let payload: Option<CommandCodeModelsResponse> = path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .or_else(|| {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("norte-code-catalog-plugin-commandcode/0.1")
                .build()
                .and_then(|client| {
                    client
                        .get(MODELS_URL)
                        .header(
                            "Authorization",
                            format!(
                                "Bearer {}",
                                std::env::var("CMD_API_KEY").unwrap_or_default()
                            ),
                        )
                        .send()
                })
                .ok()?;
            let payload = response.json::<CommandCodeModelsResponse>().ok()?;
            if let Some(path) = path.as_deref() {
                write_cache(path, now, &payload);
            }
            Some(payload)
        });
    let Some(payload) = payload else {
        return Catalogue::default();
    };

    let mut grouped: BTreeMap<&str, Vec<Model>> = BTreeMap::new();
    for model in payload.data {
        let endpoints: BTreeSet<&str> = model.supported_endpoints.iter().map(String::as_str).collect();
        for endpoint in endpoints {
            let Some(provider_id) = provider_for(endpoint) else {
                continue;
            };
            grouped
                .entry(provider_id)
                .or_default()
                .push(build_model(&model, provider_id));
        }
    }

    let mut providers = BTreeMap::new();
    let mut models = Vec::new();
    for (provider_id, mut entries) in grouped {        let name = match provider_id {
            "commandcode" => "Command Code",
            "commandcode-responses" => "Command Code (Responses)",
            "commandcode-anthropic" => "Command Code (Messages)",
            other => other,
        };
        let schema = match provider_id {
            "commandcode-responses" => WireSchema::Responses,
            "commandcode-anthropic" => WireSchema::Anthropic,
            _ => WireSchema::OpenAI,
        };
        providers.insert(
            provider_id.to_owned(),
            provider(provider_id, name, schema),
        );
        models.append(&mut entries);
    }
    Catalogue { providers, models }
}

fn provider_for(endpoint: &str) -> Option<&'static str> {
    match endpoint {
        "/chat/completions" => Some("commandcode"),
        "/responses" => Some("commandcode-responses"),
        "/messages" => Some("commandcode-anthropic"),
        _ => None,
    }
}

fn provider(_provider_id: &str, name: &str, schema: WireSchema) -> Provider {
    Provider {
        name: name.to_owned(),
        doc: Some("https://commandcode.ai/provider".to_owned()),
        environment_keys: vec!["CMD_API_KEY".to_owned()],
        wires: vec![Wire {
            api: "https://api.commandcode.ai/provider/v1".to_owned(),
            schema,
            auth: None,
            headers: BTreeMap::new(),
        }],
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

fn read_cache(path: &Path, now: u64) -> Option<CommandCodeModelsResponse> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: CachedResponse = serde_json::from_str(&text).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.payload)
}

fn write_cache(path: &Path, fetched_at: u64, payload: &CommandCodeModelsResponse) {
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
    payload: CommandCodeModelsResponse,
}

#[derive(Clone, Serialize, Deserialize)]
struct CommandCodeModelsResponse {
    data: Vec<CommandCodeModel>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CommandCodeModel {
    id: String,
    name: Option<String>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    supported_endpoints: Vec<String>,
}

fn build_model(source: &CommandCodeModel, provider_id: &str) -> Model {
    let id = source.id.clone();
    Model {
        id: id.clone(),
        provider_id: provider_id.to_owned(),
        canonical_slug: format!("{provider_id}/{id}"),
        name: source.name.clone().unwrap_or_else(|| id.clone()),
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
        limit: Limit {
            context: source.context_length,
            output: None,
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