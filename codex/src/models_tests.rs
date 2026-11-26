use norte_code_wire::catalog::{Modality, Model, SupportedEffort};
use norte_types::SupportedParam;
use serde_json::json;

use super::CodexModelsResponse;

#[test]
fn maps_only_visible_api_models_and_keeps_supported_metadata() {
    let response: CodexModelsResponse = serde_json::from_value(json!({
        "models": [
            {
                "slug": "gpt-live-model",
                "display_name": "GPT Live Model",
                "description": "Current remote model",
                "default_reasoning_level": "medium",
                "supported_reasoning_levels": [
                    {"effort": "low", "description": "Low"},
                    {"effort": "medium", "description": "Medium"},
                    {"effort": "ultra", "description": "Unsupported by Norte Code"}
                ],
                "visibility": "list",
                "supported_in_api": true,
                "context_window": 272_000,
                "input_modalities": ["text", "image", "future_modality"]
            },
            {
                "slug": "hidden-model",
                "visibility": "hide",
                "supported_in_api": true
            },
            {
                "slug": "ui-only-model",
                "visibility": "list",
                "supported_in_api": false
            }
        ]
    }))
    .expect("Codex models response");

    let models: Vec<Model> = response
        .models
        .into_iter()
        .filter_map(super::CodexModel::into_model)
        .collect();

    assert_eq!(models.len(), 1);
    let model = &models[0];
    assert_eq!(model.id, "gpt-live-model");
    assert_eq!(model.canonical_slug, "codex/gpt-live-model");
    assert_eq!(model.name, "GPT Live Model");
    assert_eq!(model.description.as_deref(), Some("Current remote model"));
    assert_eq!(model.limit.context, Some(272_000));
    assert_eq!(model.modalities.input, [Modality::Text, Modality::Image]);
    assert_eq!(model.modalities.output, [Modality::Text]);
    assert_eq!(
        model.reasoning.default_effort,
        Some(SupportedEffort::Medium)
    );
    assert_eq!(
        model.reasoning.supported_efforts,
        [SupportedEffort::Low, SupportedEffort::Medium]
    );
    assert!(model.supported_params.contains(&SupportedParam::Tools));
    assert!(
        model
            .supported_params
            .contains(&SupportedParam::ReasoningEffort)
    );
}
