//! `Codex` provider plugin for Norte Code.
//!
//! Codex (the ChatGPT harness) stores its credentials in `auth.json`, not in
//! a Norte Code-owned key. This plugin exposes the same provider through the
//! host's catalogue so a Norte Code session can drive it without asking a
//! person to type a key that is already on disk.
//!
//! The plugin never reads or writes the refresh token: `auth.json` may hold
//! one, and it is not ours to touch. Only the short-lived access token is
//! read, only at plugin start, and only to export it as `NC_CODEX_API_KEY`
//! for Norte Code to consume.
//!
//! The provider ships with no models: the model ids Codex serves are not a
//! stable list — they change with the harness and with the ChatGPT plan —
//! so the catalogue does not claim any of them by hand. When the harness
//! publishes an authoritative list, this is where it belongs.
//!
//! Everything here happens once per plugin load: no requests are made and
//! nothing is cached. If the file is missing, unreadable or malformed the
//! plugin offers nothing and stays silent; a person using another provider
//! is not told about a Codex they did not configure.

use std::collections::BTreeMap;
use std::path::PathBuf;

use norte_code_wire::catalog::{Provider, Wire, WireSchema};
use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::Deserialize;

const PROVIDER_ID: &str = "codex";
const PROVIDER_NAME: &str = "Codex";
const PROVIDER_DOC: &str = "https://chatgpt.com/backend-api/codex";
const API_URL: &str = "https://chatgpt.com/backend-api/codex";
const ENV_KEY: &str = "NC_CODEX_API_KEY";

/// A plugin that exposes the Codex provider through the host.
#[derive(Default)]
pub struct CodexPlugin;

impl Plugin for CodexPlugin {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn catalogue(&self) -> Catalogue {
        let Some(token) = read_access_token() else {
            return Catalogue::default();
        };
        // SAFETY: `set_var` is unsafe in Rust 2024 because mutating the environment
        // of a live process is not race-free. This library is loaded into the
        // plugin host before any worker threads race on the environment; it is
        // set exactly once at startup and never mutated again.
        unsafe {
            std::env::set_var(ENV_KEY, token);
        }
        catalogue()
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
    Box::into_raw(Box::new(CodexPlugin))
}

/// Reads the short-lived access token from Codex's `auth.json`.
///
/// The file is `$CODEX_HOME/auth.json`, or `~/.codex/auth.json` when
/// `CODEX_HOME` is unset. A missing, unreadable or malformed file yields
/// `None`; this plugin offers nothing in that case rather than guessing.
fn read_access_token() -> Option<String> {
    for path in auth_paths() {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<CodexAuth>(&content) else {
            continue;
        };
        let token = parsed.access_token.trim().to_owned();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}

fn auth_paths() -> Vec<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return vec![PathBuf::from(home).join("auth.json")];
    }
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    vec![PathBuf::from(home).join(".codex").join("auth.json")]
}

#[derive(Deserialize)]
struct CodexAuth {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    #[allow(dead_code)]
    refresh_token: String,
}

fn catalogue() -> Catalogue {
    let providers = BTreeMap::from([(PROVIDER_ID.to_owned(), provider())]);
    // The catalogue is empty by design: Codex's offered model ids are not
    // static — they follow the ChatGPT plan and the harness' own release
    // calendar — so the catalogue does not pin any of them here. A release
    // landing in the CLI is visible once the plugin merges an authoritative
    // list; hard-coding names here would leave stale entries behind.
    let models = Vec::new();
    Catalogue { providers, models }
}

fn provider() -> Provider {
    Provider {
        name: PROVIDER_NAME.to_owned(),
        doc: Some(PROVIDER_DOC.to_owned()),
        environment_keys: vec![ENV_KEY.to_owned()],
        wires: vec![Wire {
            api: API_URL.to_owned(),
            schema: WireSchema::Responses,
            auth: None,
            headers: std::collections::BTreeMap::new(),
        }],
    }
}