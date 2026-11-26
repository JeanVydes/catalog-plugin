use norte_code_wire::catalog::{Modality, ModelStatus, WireSchema};
use norte_types::SupportedParam;

use super::{OpenAiModel, OpenAiModelsResponse, is_chat_model, model, provider};

#[test]
fn keeps_models_owned_by_an_organization() {
    let response = serde_json::from_str::<OpenAiModelsResponse>(
        r#"{"data":[{"id":"gpt-4.1-custom","owned_by":"organization-owner","shutdown_date":null}]}"#,
    )
    .expect("documented model-list response should deserialize");
    assert_eq!(response.data.len(), 1);
    assert_eq!(
        response.data[0].owned_by.as_deref(),
        Some("organization-owner")
    );
    assert!(is_chat_model(&response.data[0]));
}

#[test]
fn excludes_non_chat_api_models() {
    for id in [
        "text-embedding-3-large",
        "gpt-4o-realtime-preview",
        "gpt-image-1",
        "gpt-3.5-turbo-instruct",
        "davinci-002",
        "babbage-002",
        "computer-use-preview",
    ] {
        assert!(!is_chat_model(&OpenAiModel {
            id: id.to_owned(),
            owned_by: None,
            shutdown_date: None,
        }));
    }
}

#[test]
fn maps_only_metadata_the_models_endpoint_can_support() {
    let mapped = model(OpenAiModel {
        id: "gpt-4.1".to_owned(),
        owned_by: Some("openai".to_owned()),
        shutdown_date: Some("2027-01-01".to_owned()),
    });
    assert_eq!(mapped.id, "gpt-4.1");
    assert_eq!(mapped.status, Some(ModelStatus::Deprecated));
    assert_eq!(mapped.modalities.input, [Modality::Text]);
    assert!(mapped.limit.context.is_none());
    assert_eq!(mapped.reasoning.supported_efforts, Vec::new());
    assert!(mapped.supported_params.contains(&SupportedParam::Tools));
}

#[test]
fn provider_uses_openai_chat_endpoint() {
    let provider = provider();
    assert_eq!(provider.wires[0].schema, WireSchema::OpenAI);
    assert_eq!(provider.wires[0].api, "https://api.openai.com/v1");
}
