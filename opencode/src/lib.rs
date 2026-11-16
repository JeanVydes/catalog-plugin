//! `OpenCode` provider plugin for Norte Code.
//!
//! `OpenCode` publishes one inference endpoint and a `/v1/models` list with the
//! catalogue it serves. That list is read at plugin load and cached for 15
//! minutes; it is the same snapshot the console renders.
//!
//! Models come in families: `chat-completions`, `responses` and `messages`.
//! Each model belongs to exactly one of them. The plugin emits one provider
//! for each so a model never rides the wrong wire:
//!
//! | Provider | Wire | Family |
//! | --- | --- | --- |
//! | `opencode` | `openai` | `chat-completions` |
//! | `opencode-responses` | `responses` | `responses` |
//! | `opencode-messages` | `anthropic` | `messages` |
//!
//! Authentication uses `OPENCODE_API_KEY`, matching the console's key.

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

const MODELS_URL: &str = "https://opencode.ai/inference/v1/models";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const CACHE_FILE: &str = "opencode-models.json";

#[derive(Default)]
struct OpenCodePlugin {
    catalogue: OnceLock<Catalogue>,
}

impl Plugin for OpenCodePlugin {
    fn name(&self) -> &'static str {
        "opencode"
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
    Box::into_raw(Box::new(OpenCodePlugin::default()))
}

fn fetch_catalogue() -> Catalogue {
    let now = unix_now();
    let path = cache_path();
    let payload: Option<OpenCodeModelsResponse> = path
        .as_deref()
        .and_then(|path| read_cache(path, now))
        .or_else(|| {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("norte-code-catalog-plugin-opencode/0.1")
                .build()
                .and_then(|client| {
                    let mut request = client.get(MODELS_URL);
                    if let Ok(key) = std::env::var("OPENCODE_API_KEY") {
                        if !key.trim().is_empty() {
                            request = request.header("Authorization", format!("Bearer {}", key));
                        }
                    }
                    request.send()
                })
                .ok()?;
            let payload = response.json::<OpenCodeModelsResponse>().ok()?;
            if let Some(path) = path.as_deref() {
                write_cache(path, now, &payload);
            }
            Some(payload)
        });
    let Some(payload) = payload else {
        return Catalogue::default();
    };
    let mut providers = BTreeMap::new();
    let mut models = Vec::new();
    for (provider_id, provider_name, api_base, schema, family) in [
        (
            "opencode",
            "OpenCode",
            "https://opencode.ai/inference/openai/v1",
            WireSchema::OpenAI,
            "chat-completions",
        ),
        (
            "opencode-responses",
            "OpenCode (Responses)",
            "https://opencode.ai/inference/openai/v1",
            WireSchema::Responses,
            "responses",
        ),
        (
            "opencode-messages",
            "OpenCode (Messages)",
            "https://opencode.ai/inference/anthropic/v1",
            WireSchema::Anthropic,
            "messages",
        ),
    ] {
        let matched: Vec<&OpenCodeModel> = payload
            .data
            .iter()
            .filter(|model| model.api.contains_key(family))
            .collect();
        if matched.is_empty() {
            continue;
        }
        providers.insert(
            provider_id.to_owned(),
            provider(provider_id, provider_name, api_base, schema),
        );
        models.extend(matched.into_iter().map(|source| build_model(source, provider_id)));
    }
    Catalogue { providers, models }
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

fn read_cache(path: &Path, now: u64) -> Option<OpenCodeModelsResponse> {
    let text = std::fs::read_to_string(path).ok()?;
    let cache: CachedResponse = serde_json::from_str(&text).ok()?;
    (now.saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()).then_some(cache.payload)
}

fn write_cache(path: &Path, fetched_at: u64, payload: &OpenCodeModelsResponse) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let cache = CachedResponse {
        fetched_at,
        payload: (*payload).clone(),
    };
    let Ok(text) = serde_json::to_string(&cache) else {
        return;
    };
    let _ = std::fs::write(path, text);
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedResponse {
    fetched_at: u64,
    payload: OpenCodeModelsResponse,
}

#[derive(Clone, Serialize, Deserialize)]
struct OpenCodeModelsResponse {
    data: Vec<OpenCodeModel>,
}

#[derive(Clone, Serialize, Deserialize)]
struct OpenCodeModel {
    id: String,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    modalities: Option<ModalitiesDescriptor>,
    #[serde(default)]
    capabilities: Option<CapabilitiesDescriptor>,
    #[serde(default)]
    limit: Option<LimitDescriptor>,
    #[serde(default)]
    open_weights: bool,
    #[serde(default)]
    api: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ModalitiesDescriptor {
    input: Vec<String>,
    output: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CapabilitiesDescriptor {
    #[serde(default)]
    tools: Option<ToolDescriptor>,
    #[serde(default)]
    reasoning: Option<ReasoningDescriptor>,
    #[serde(default)]
    structured_output: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct ToolDescriptor {
    supported: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct ReasoningDescriptor {
    supported: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct LimitDescriptor {
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
}

fn provider(_provider_id: &str, name: &str, api: &str, schema: WireSchema) -> Provider {
    Provider {
        name: name.to_owned(),
        doc: Some("https://opencode.ai/docs/console".to_owned()),
        environment_keys: vec!["OPENCODE_API_KEY".to_owned()],
        wires: vec![Wire {
            api: api.to_owned(),
            schema,
            auth: None,
            headers: BTreeMap::new(),
        }],
    }
}

fn build_model(source: &OpenCodeModel, provider_id: &str) -> Model {
    let id = source.id.clone();
    let input_modalities = source.modalities.as_ref().map_or_else(
        || vec![Modality::Text],
        |modalities| modalities_of(&modalities.input),
    );
    let output_modalities = source.modalities.as_ref().map_or_else(
        || vec![Modality::Text],
        |modalities| modalities_of(&modalities.output),
    );
    let capabilities = source.capabilities.as_ref();
    let tools_supported = capabilities
        .and_then(|caps| caps.tools.as_ref())
        .is_some_and(|tool| tool.supported);
    let reasoning_supported = capabilities
        .and_then(|caps| caps.reasoning.as_ref())
        .is_some_and(|reasoning| reasoning.supported);
    let structured_supported =
        capabilities.is_some_and(|caps| caps.structured_output);
    let mut supported_params = Vec::new();
    if tools_supported {
        supported_params.push(SupportedParams::Tools);
        supported_params.push(SupportedParams::ToolChoice);
    }
    if structured_supported {
        supported_params.push(SupportedParams::StructuredOutput);
    }
    if reasoning_supported {
        supported_params.push(SupportedParams::Reasoning);
    }
    Model {
        id: id.clone(),
        provider_id: provider_id.to_owned(),
        canonical_slug: format!("{provider_id}/{id}"),
        name: source.name.clone().unwrap_or_else(|| source.id.clone()),
        description: source.description.clone(),
        family: source.family.clone(),
        knowledge: None,
        release_date: None,
        last_updated: None,
        reasoning: Reasoning {
            mandatory: false,
            default_enabled: reasoning_supported,
            default_effort: None,
            supported_efforts: Vec::new(),
            budget_supported: false,
        },
        modalities: Modalities {
            input: input_modalities,
            output: output_modalities,
        },
        open_weights: source.open_weights,
        limit: Limit {
            context: source.limit.as_ref().and_then(|limit| limit.context),
            output: source.limit.as_ref().and_then(|limit| limit.output),
        },
        status: None,
        supported_params,
        pricing: None,
    }
}

fn modalities_of(values: &[String]) -> Vec<Modality> {
    values
        .iter()
        .filter_map(|value| match value.as_str() {
            "text" => Some(Modality::Text),
            "image" => Some(Modality::Image),
            "audio" => Some(Modality::Audio),
            "video" => Some(Modality::Video),
            "file" | "pdf" => Some(Modality::File),
            _ => None,
        })
        .collect()
}