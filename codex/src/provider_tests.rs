use super::*;

#[test]
fn responses_wire_declares_the_cache_breakpoint_exception() {
    let provider = provider();
    let wire = provider
        .wire(WireSchema::Responses)
        .expect("Responses wire");
    assert!(
        wire.quirks
            .contains(&norte_types::Quirk::NoPromptCacheBreakpoints)
    );
}
