use std::path::PathBuf;

use serde::Deserialize;

use crate::executable;

pub(super) fn read_auth() -> Option<CodexAuth> {
    auth_paths()
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
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

#[derive(Default, Deserialize)]
pub(super) struct CodexAuth {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    tokens: Option<CodexTokens>,
}

#[derive(Default, Deserialize)]
struct CodexTokens {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    account_id: Option<String>,
}

impl CodexAuth {
    pub(super) fn access_token(&self) -> Option<&str> {
        self.tokens
            .as_ref()
            .map(|tokens| tokens.access_token.as_str())
            .filter(|token| !token.trim().is_empty())
            .or_else(|| {
                (!self.access_token.trim().is_empty()).then_some(self.access_token.as_str())
            })
    }

    pub(super) fn account_id(&self) -> Option<&str> {
        self.tokens
            .as_ref()
            .and_then(|tokens| tokens.account_id.as_deref())
            .or(self.account_id.as_deref())
            .filter(|account_id| !account_id.trim().is_empty())
    }
}

pub(super) fn client_version() -> Option<String> {
    cached_client_version().or_else(installed_client_version)
}

fn cached_client_version() -> Option<String> {
    let path = codex_home()?.join("models_cache.json");
    let content = std::fs::read_to_string(path).ok()?;
    let cache: CodexModelsCache = serde_json::from_str(&content).ok()?;
    normalize_version(&cache.client_version?)
}

fn installed_client_version() -> Option<String> {
    let output = executable::run(|command| command.arg("--version").output()).ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    version.split_whitespace().find_map(normalize_version)
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
}

fn normalize_version(value: &str) -> Option<String> {
    let mut components = value.trim().split('.');
    let major = version_component(components.next()?)?;
    let minor = version_component(components.next()?)?;
    let patch = version_component(components.next()?)?;
    Some(format!("{major}.{minor}.{patch}"))
}

fn version_component(value: &str) -> Option<&str> {
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0).then_some(&value[..digits])
}

#[derive(Default, Deserialize)]
struct CodexModelsCache {
    #[serde(default)]
    client_version: Option<String>,
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
