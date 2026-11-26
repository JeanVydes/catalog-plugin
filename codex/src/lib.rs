//! `Codex` provider plugin for Norte Code.

mod api;
mod auth;
mod models;
mod usage;
mod usage_widget;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin, PluginContext};
use norte_code_plugin_host::{PluginInvocation, WidgetDefinition};
use norte_code_wire::catalog::{Provider, Wire, WireSchema};
use norte_code_wire::plugin_widgets::{WidgetId, WidgetView};

const USAGE_WIDGET: &str = "usage";
const PROVIDER_ID: &str = "codex";
const PROVIDER_NAME: &str = "Codex";
const PROVIDER_DOC: &str = "https://chatgpt.com/backend-api/codex";
const API_URL: &str = "https://chatgpt.com/backend-api/codex";
const ENV_KEY: &str = "NC_CODEX_API_KEY";

/// A plugin that exposes the Codex provider and its current model catalogue.
#[derive(Default)]
pub struct CodexPlugin {
    catalogue: OnceLock<Catalogue>,
    account: OnceLock<String>,
    context: Option<PluginContext>,
    usage: usage_widget::UsageWidget,
}

impl Plugin for CodexPlugin {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn catalogue(&self) -> Catalogue {
        self.catalogue
            .get_or_init(|| fetch_catalogue(&self.account))
            .clone()
    }
    fn set_context(&mut self, context: PluginContext) {
        self.context = Some(context);
    }

    fn widgets(&self) -> Vec<WidgetDefinition> {
        vec![WidgetDefinition {
            id: WidgetId::new(USAGE_WIDGET),
            title: "Codex usage".to_owned(),
        }]
    }

    fn widget(&self, id: &WidgetId, invocation: &PluginInvocation) -> Option<WidgetView> {
        let selection = &invocation.selection;
        if id.as_str() != USAGE_WIDGET || selection.provider.as_deref() != Some(PROVIDER_ID) {
            return None;
        }
        let model = selection.model.as_deref()?;
        let context = self.context.as_ref()?;
        self.catalogue
            .get()?
            .models
            .iter()
            .any(|entry| entry.id == model)
            .then(|| {
                self.usage
                    .show(context, self.account.get().cloned(), id.clone())
            })
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
    Box::into_raw(Box::new(CodexPlugin::default()))
}

fn fetch_catalogue(account: &OnceLock<String>) -> Catalogue {
    let Some(auth) = auth::read_auth() else {
        return Catalogue::default();
    };
    let Some(token) = auth.access_token() else {
        return Catalogue::default();
    };

    if let Some(account_id) = auth.account_id() {
        let _ = account.set(account_id.to_owned());
    }

    // SAFETY: the host loads plugins before starting worker threads. The token
    // is installed once during catalogue construction and never changed.
    unsafe {
        std::env::set_var(ENV_KEY, token);
    }

    let models = auth::client_version()
        .and_then(|version| api::fetch_models(token, auth.account_id(), &version))
        .map(|response| {
            response
                .models
                .into_iter()
                .filter_map(models::CodexModel::into_model)
                .collect()
        })
        .unwrap_or_default();

    Catalogue {
        providers: BTreeMap::from([(PROVIDER_ID.to_owned(), provider())]),
        models,
    }
}

fn provider() -> Provider {
    Provider {
        name: PROVIDER_NAME.to_owned(),
        doc: Some(PROVIDER_DOC.to_owned()),
        environment_keys: vec![ENV_KEY.to_owned()],
        wires: vec![Wire {
            quirks: [norte_types::Quirk::NoPromptCacheBreakpoints].into(),
            api: API_URL.to_owned(),
            schema: WireSchema::Responses,
            auth: None,
            headers: BTreeMap::new(),
        }],
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod provider_tests;
