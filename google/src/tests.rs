use norte_code_wire::catalog::{Modality, SupportedParams, WireSchema};

use super::{GoogleModel, is_generate_content_model, model, model_id, provider};

#[test]
fn only_lists_models_that_can_generate_content() {
    assert!(is_generate_content_model(&source(
        "models/gemini-2.5-flash",
        Some("gemini-2.5-flash"),
        &["generateContent", "countTokens"],
        false,
    )));
    assert!(!is_generate_content_model(&source(
        "models/text-embedding-004",
        None,
        &["embedContent"],
        false,
    )));
}

#[test]
fn uses_base_model_id_and_api_token_limits() {
    let mapped = model(source(
        "models/gemini-2.5-flash-001",
        Some("gemini-2.5-flash"),
        &["generateContent"],
        true,
    ));
    assert_eq!(mapped.id, "gemini-2.5-flash");
    assert_eq!(mapped.limit.context, Some(1_000_000));
    assert_eq!(mapped.limit.output, Some(65_536));
    assert_eq!(mapped.modalities.input, [Modality::Text]);
    assert!(!mapped.reasoning.default_enabled);
    assert!(mapped.reasoning.budget_supported);
    assert!(
        mapped
            .supported_params
            .contains(&SupportedParams::Reasoning)
    );
    assert!(mapped.supported_params.contains(&SupportedParams::Tools));
}

#[test]
fn falls_back_to_the_resource_name_when_base_id_is_absent() {
    assert_eq!(model_id("models/gemini-2.5-flash"), "gemini-2.5-flash");
}

#[test]
fn provider_uses_versioned_generate_content_base_url() {
    let provider = provider();
    assert_eq!(provider.wires[0].schema, WireSchema::Google);
    assert_eq!(
        provider.wires[0].api,
        "https://generativelanguage.googleapis.com/v1beta"
    );
}

fn source(
    name: &str,
    base_model_id: Option<&str>,
    methods: &[&str],
    thinking: bool,
) -> GoogleModel {
    GoogleModel {
        name: name.to_owned(),
        base_model_id: base_model_id.map(str::to_owned),
        display_name: Some("Gemini test model".to_owned()),
        description: Some("A test model".to_owned()),
        input_token_limit: Some(1_000_000),
        output_token_limit: Some(65_536),
        supported_generation_methods: methods.iter().map(|method| (*method).to_owned()).collect(),
        thinking,
    }
}
