use serde_json::json;

use super::{CodexAuth, normalize_version};

#[test]
fn reads_access_token_and_account_from_current_auth_shape() {
    let auth: CodexAuth = serde_json::from_value(json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": "test-access-token",
            "account_id": "test-account"
        }
    }))
    .expect("current Codex auth shape");

    assert_eq!(auth.access_token(), Some("test-access-token"));
    assert_eq!(auth.account_id(), Some("test-account"));
}

#[test]
fn reads_legacy_top_level_access_token() {
    let auth: CodexAuth = serde_json::from_value(json!({
        "access_token": "legacy-access-token",
        "account_id": "legacy-account"
    }))
    .expect("legacy Codex auth shape");

    assert_eq!(auth.access_token(), Some("legacy-access-token"));
    assert_eq!(auth.account_id(), Some("legacy-account"));
}

#[test]
fn normalizes_codex_prerelease_versions_for_catalog_request() {
    let version = "codex-cli 0.171.0-alpha.13.2"
        .split_whitespace()
        .find_map(normalize_version);
    assert_eq!(version.as_deref(), Some("0.171.0"));
    assert_eq!(normalize_version("not-a-version"), None);
}
