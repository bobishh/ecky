use ecky_cad_lib::provider_turn::{
    ProviderTurnIntent, ProviderTurnPolicy, UNIFIED_TURN_POLICY_PROMPT,
};

#[test]
fn prompt_based_policy_contains_all_turn_categories_in_one_prompt() {
    let contract = ProviderTurnPolicy::unified_prompt_contract();
    assert!(contract.contains("ANSWER"));
    assert!(contract.contains("INSPECT"));
    assert!(contract.contains("MODIFY"));
    assert!(contract.contains("CLARIFY"));
    assert_eq!(contract, UNIFIED_TURN_POLICY_PROMPT);
}

#[test]
fn prompt_based_turn_wraps_user_message_with_unified_policy() {
    let policy = ProviderTurnPolicy::prompt_based();
    let prompt = policy.wrap_user_message("делай коробку 10x10 или ответь почему не получается");
    assert!(prompt.contains(UNIFIED_TURN_POLICY_PROMPT));
    assert!(prompt.contains("[USER MESSAGE]"));
    assert!(prompt.ends_with("делай коробку 10x10 или ответь почему не получается"));
}

#[test]
fn prompt_based_turn_allows_tools_for_model_execution() {
    let policy = ProviderTurnPolicy::prompt_based();
    assert!(policy.allows_any_tool());
    assert!(policy.allows_project_writes());
    assert!(policy.allows_mcp_tool("macro_preview_render"));
    assert!(policy.allows_mcp_tool("workspace_overview"));
    assert_eq!(policy.execution_mode(), "accept-edits");
}

#[test]
fn explicit_policy_still_constrains_tools_when_specified() {
    let answer = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer);
    assert!(!answer.allows_any_tool());
    assert!(!answer.allows_project_writes());

    let inspect = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Inspect);
    assert!(inspect.allows_any_tool());
    assert!(!inspect.allows_project_writes());
    assert!(inspect.allows_mcp_tool("workspace_overview"));
    assert!(!inspect.allows_mcp_tool("macro_preview_render"));

    let modify = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify);
    assert!(modify.allows_any_tool());
    assert!(modify.allows_project_writes());
    assert!(modify.allows_mcp_tool("macro_preview_render"));

    let clarify = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Clarify);
    assert!(!clarify.allows_project_writes());
}

#[test]
fn execution_mode_is_never_plan() {
    assert_eq!(
        ProviderTurnPolicy::prompt_based().execution_mode(),
        "accept-edits"
    );
    assert_eq!(
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify).execution_mode(),
        "accept-edits"
    );
    assert_eq!(
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Inspect).execution_mode(),
        "accept-edits"
    );
    assert_eq!(
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer).execution_mode(),
        "accept-edits"
    );
    assert_eq!(
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Clarify).execution_mode(),
        "accept-edits"
    );
}
