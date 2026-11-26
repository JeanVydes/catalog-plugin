//! Anthropic provider plugin for Norte Code.

mod catalog;

use std::sync::OnceLock;

use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};

#[derive(Default)]
struct AnthropicPlugin {
    state: OnceLock<PluginState>,
}

impl AnthropicPlugin {
    fn state(&self) -> &PluginState {
        self.state.get_or_init(|| match catalog::fetch_catalogue() {
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

impl Plugin for AnthropicPlugin {
    fn name(&self) -> &'static str {
        "anthropic"
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
    Box::into_raw(Box::new(AnthropicPlugin::default()))
}
