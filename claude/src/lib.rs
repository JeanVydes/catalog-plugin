//! `Claude Code` provider plugin for Norte Code.
//!
//! Claude Code stores its OAuth access token in `.credentials.json` beside the
//! user's `CLAUDE_HOME` directory, not in a Norte Code-owned key. This plugin
//! exposes the same provider through the host's catalogue, with the same
//! caveats as the Codex plugin: no refresh token is read or touched, and the
//! catalogue does not invent model ids — `CLAUDE_HOME/.credentials.json` does
//! not advertise what Codex/the CLI knows about them.
//!
//! Everything here happens once per plugin load: no requests are made, and
//! nothing is cached. If the credentials file is missing, unreadable or
//! malformed the plugin offers nothing and stays silent.

use std::collections::BTreeMap;
use std::path::PathBuf;

use norte_code_wire::catalog::{AuthKind, Provider, Wire, WireSchema};
use norte_code_plugin_host::plugins::{Catalogue, PLUGIN_ABI_VERSION, Plugin};
use serde::Deserialize;

const PROVIDER_ID: &str = "claude-code";
const PROVIDER_NAME: &str = "Claude Code";
const PROVIDER_DOC: &str = "https://claude.ai/code";
const API_URL: &str = "https://api.anthropic.com";
const ENV_KEY: &str = "NC_CLAUDE_CODE_API_KEY";

/// A plugin that exposes the Claude Code OAuth token as a provider key.
#[derive(Default)]
struct ClaudePlugin;

impl Plugin for ClaudePlugin {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn catalogue(&self) -> Catalogue {
        let Some(token) = read_access_token() else {
            return Catalogue::default();
        };
        // SAFETY: `set_var` is unsafe in Rust 2024 because mutating the
        // environment of a live process is not race-free. This library is
        // loaded before any calls race on the environment, and it sets the
        // variable exactly once at startup; it is never mutated afterward.
        unsafe {
            std::env::set_var(ENV_KEY, token);
        }
        Catalogue {
            providers: BTreeMap::from([(PROVIDER_ID.to_owned(), provider())]),
            models: Vec::new(),
        }
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
    Box::into_raw(Box::new(ClaudePlugin))
}

/// Reads the short-lived access token from Claude Code's credentials file.
///
/// The file is `$CLAUDE_HOME/.credentials.json` when the variable is set,
/// otherwise `~/.claude/.credentials.json`. `accessToken` at either the top
/// level or under `claudeAiOauth` is accepted; anything without one yields
/// `None` and the plugin emits nothing.
fn read_access_token() -> Option<String> {
    for path in credential_paths() {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<ClaudeCodeAuth>(&content) else {
            continue;
        };
        let token = parsed.access_token();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}

fn credential_paths() -> Vec<PathBuf> {
    if let Some(home) = std::env::var_os("CLAUDE_HOME") {
        return vec![PathBuf::from(home).join(".credentials.json")];
    }
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    vec![PathBuf::from(home).join(".claude").join(".credentials.json")]
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCodeAuth {
    #[serde(default)]
    access_token: String,
    claude_ai_oauth: Option<NestedTokens>,
}

impl ClaudeCodeAuth {
    fn access_token(self) -> String {
        if !self.access_token.trim().is_empty() {
            return self.access_token.trim().to_owned();
        }
        self.claude_ai_oauth
            .map(|tokens| tokens.access_token)
            .unwrap_or_default()
            .trim()
            .to_owned()
    }
}

#[derive(Deserialize)]
struct NestedTokens {
    #[serde(default, rename = "accessToken")]
    access_token: String,
}

fn provider() -> Provider {
    Provider {
        name: PROVIDER_NAME.to_owned(),
        doc: Some(PROVIDER_DOC.to_owned()),
        environment_keys: vec![ENV_KEY.to_owned()],
        wires: vec![Wire {
            api: API_URL.to_owned(),
            schema: WireSchema::Anthropic,
            auth: Some(AuthKind::Bearer),
            headers: BTreeMap::new(),
        }],
    }
}