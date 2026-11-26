use norte_code_wire::catalog::{Modality, ModelStatus, SupportedEffort, WireSchema};
use norte_types::SupportedParam;

use super::{
    AnthropicModel, CapabilitySupport, EffortCapability, ModelCapabilities, ModelLifecycle,
    ThinkingCapability, ThinkingTypes, model, provider,
};

#[test]
fn maps_only_published_model_limits_and_capabilities() {
    let mapped = model(AnthropicModel {
        id: "claude-sonnet-example".to_owned(),
        display_name: Some("Claude Sonnet Example".to_owned()),
        max_input_tokens: Some(200_000),
        max_tokens: Some(64_000),
        capabilities: Some(ModelCapabilities {
            image_input: Some(CapabilitySupport { supported: true }),
            pdf_input: Some(CapabilitySupport { supported: true }),
            structured_outputs: Some(CapabilitySupport { supported: true }),
            thinking: Some(ThinkingCapability {
                supported: true,
                types: Some(ThinkingTypes {
                    adaptive: Some(CapabilitySupport { supported: true }),
                    disabled: Some(CapabilitySupport { supported: false }),
                    enabled: Some(CapabilitySupport { supported: false }),
                }),
            }),
            effort: Some(EffortCapability {
                supported: true,
                low: Some(CapabilitySupport { supported: true }),
                medium: Some(CapabilitySupport { supported: true }),
                high: Some(CapabilitySupport { supported: true }),
                xhigh: None,
                max: None,
            }),
        }),
        lifecycle: ModelLifecycle::Active,
        line: Some("sonnet".to_owned()),
    });
    assert_eq!(
        mapped.modalities.input,
        [Modality::Text, Modality::Image, Modality::File]
    );
    assert_eq!(mapped.limit.context, Some(200_000));
    assert_eq!(mapped.limit.output, Some(64_000));
    assert_eq!(mapped.family.as_deref(), Some("sonnet"));
    assert!(!mapped.reasoning.default_enabled);
    assert!(mapped.reasoning.mandatory);
    assert_eq!(
        mapped.reasoning.supported_efforts,
        [
            SupportedEffort::Low,
            SupportedEffort::Medium,
            SupportedEffort::High
        ]
    );
    assert!(mapped.supported_params.contains(&SupportedParam::Tools));
    assert!(
        mapped
            .supported_params
            .contains(&SupportedParam::StructuredOutput)
    );
}

#[test]
fn marks_deprecated_models_and_does_not_invent_capabilities() {
    let mapped = model(AnthropicModel {
        id: "claude-legacy".to_owned(),
        display_name: None,
        max_input_tokens: Some(0),
        max_tokens: None,
        capabilities: None,
        lifecycle: ModelLifecycle::Deprecated,
        line: None,
    });
    assert_eq!(mapped.status, Some(ModelStatus::Deprecated));
    assert_eq!(mapped.limit.context, None);
    assert_eq!(mapped.modalities.input, [Modality::Text]);
    assert!(!mapped.reasoning.default_enabled);
    assert!(!mapped.reasoning.mandatory);
    assert!(
        !mapped
            .supported_params
            .contains(&SupportedParam::StructuredOutput)
    );
}

#[test]
fn maps_explicit_thinking_types_without_inventing_a_default() {
    let mapped = model(AnthropicModel {
        id: "claude-sonnet-thinking".to_owned(),
        display_name: None,
        max_input_tokens: None,
        max_tokens: None,
        capabilities: Some(ModelCapabilities {
            thinking: Some(ThinkingCapability {
                supported: true,
                types: Some(ThinkingTypes {
                    adaptive: Some(CapabilitySupport { supported: false }),
                    disabled: Some(CapabilitySupport { supported: true }),
                    enabled: Some(CapabilitySupport { supported: true }),
                }),
            }),
            ..ModelCapabilities::default()
        }),
        lifecycle: ModelLifecycle::Active,
        line: None,
    });
    assert!(!mapped.reasoning.default_enabled);
    assert!(!mapped.reasoning.mandatory);
    assert!(mapped.reasoning.budget_supported);
    assert_eq!(mapped.reasoning.supported_efforts, [SupportedEffort::None]);
}

#[test]
fn provider_uses_anthropic_messages_api() {
    let provider = provider(None);
    assert_eq!(provider.wires[0].schema, WireSchema::Anthropic);
    assert_eq!(provider.wires[0].api, "https://api.anthropic.com");
    assert!(provider.wires[0].headers.is_empty());
}

#[test]
fn sends_configured_workspace_id_on_catalogue_and_model_wires() {
    let provider = provider(Some("workspace-example"));
    assert_eq!(
        provider.wires[0]
            .headers
            .get("anthropic-workspace-id")
            .map(String::as_str),
        Some("workspace-example")
    );
}
