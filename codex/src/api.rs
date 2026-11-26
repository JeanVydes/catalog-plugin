use std::time::Duration;

use super::models::CodexModelsResponse;

const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn fetch_models(
    token: &str,
    account_id: Option<&str>,
    version: &str,
) -> Option<CodexModelsResponse> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(format!(
            "norte-code-catalog-plugin-codex/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .ok()?;
    let mut request = client
        .get(MODELS_URL)
        .query(&[("client_version", version)])
        .header("Authorization", format!("Bearer {token}"))
        .header("originator", "norte_code")
        .header("Accept", "application/json");
    if let Some(account_id) = account_id {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    request.send().ok()?.error_for_status().ok()?.json().ok()
}
