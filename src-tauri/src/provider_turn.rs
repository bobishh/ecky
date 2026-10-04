use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderTurnIntent {
    Answer,
    Plan,
    Inspect,
    Modify,
    Clarify,
}

impl ProviderTurnIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Plan => "plan",
            Self::Inspect => "inspect",
            Self::Modify => "modify",
            Self::Clarify => "clarify",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "answer" => Some(Self::Answer),
            "plan" => Some(Self::Plan),
            "inspect" => Some(Self::Inspect),
            "modify" => Some(Self::Modify),
            "clarify" => Some(Self::Clarify),
            _ => None,
        }
    }
}

pub const UNIFIED_TURN_POLICY_PROMPT: &str = "[TURN POLICY]\n\
Categorize the user's message and follow the corresponding mode:\n\
- ANSWER: If the user is asking a general question, requesting status, or seeking explanation, answer directly from conversation context. Do not call MCP tools. Do not inspect or edit project files.\n\
- INSPECT: If the user asks to check, measure, view, or inspect existing state or geometry, use only read-only inspection tools. Do not edit project files.\n\
- MODIFY: If the user explicitly requests CAD or geometry changes, additions, or fixes, perform the change: inspect, edit, preview, and verify.\n\
- CLARIFY: Ask one concise question only when a fact strictly needed to act is missing and cannot be found in current project state or recent dialogue, or resolved by a reasonable reversible default. Do not ask again for a choice the user already made.\n\
The current user message controls this turn. A confirmation such as yes, proceed, or a repeated imperative authorizes the proposal in recent dialogue. Earlier unfinished work alone does not authorize a new change.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTurnPolicy {
    explicit_intent: Option<ProviderTurnIntent>,
    answer_first: bool,
}

impl ProviderTurnPolicy {
    pub fn prompt_based() -> Self {
        Self {
            explicit_intent: None,
            answer_first: false,
        }
    }

    pub fn for_intent(intent: ProviderTurnIntent) -> Self {
        Self {
            explicit_intent: Some(intent),
            answer_first: false,
        }
    }

    pub fn routed(intent: ProviderTurnIntent, answer_first: bool) -> Self {
        Self {
            explicit_intent: Some(intent),
            answer_first,
        }
    }

    pub fn requires_answer_first(self) -> bool {
        self.answer_first
    }

    pub fn unified_prompt_contract() -> &'static str {
        UNIFIED_TURN_POLICY_PROMPT
    }

    pub fn is_prompt_based(self) -> bool {
        self.explicit_intent.is_none()
    }

    pub fn allows_any_tool(self) -> bool {
        matches!(
            self.explicit_intent,
            Some(ProviderTurnIntent::Inspect | ProviderTurnIntent::Modify) | None
        )
    }

    pub fn allows_project_writes(self) -> bool {
        matches!(
            self.explicit_intent,
            Some(ProviderTurnIntent::Modify) | None
        )
    }

    pub fn intent(self) -> ProviderTurnIntent {
        self.explicit_intent.unwrap_or(ProviderTurnIntent::Modify)
    }

    pub fn execution_mode(self) -> &'static str {
        "accept-edits"
    }

    pub fn prompt_contract(self) -> &'static str {
        match self.explicit_intent {
            None => UNIFIED_TURN_POLICY_PROMPT,
            Some(ProviderTurnIntent::Answer) => {
                "[TURN POLICY]\nIntent: ANSWER\nAnswer the current user message immediately from supplied conversation context. Do not call tools. Do not inspect or edit project files."
            }
            Some(ProviderTurnIntent::Plan) => {
                "[TURN POLICY]\nIntent: PLAN\nDescribe a proposed approach only. Do not call tools, inspect project files, enter built-in planning mode, or modify project state."
            }
            Some(ProviderTurnIntent::Inspect) => {
                "[TURN POLICY]\nIntent: INSPECT\nUse only advertised read-only tools. Do not edit project files or resume unfinished authoring. Return the answer after bounded inspection."
            }
            Some(ProviderTurnIntent::Modify) => {
                "[TURN POLICY]\nIntent: MODIFY\nPerform the change requested in the current user message. Inspect, edit, preview, and verify."
            }
            Some(ProviderTurnIntent::Clarify) => {
                "[TURN POLICY]\nIntent: CLARIFY\nAsk one concise question for the missing critical requirement. Do not call tools, inspect or modify project state, or self-escalate to another intent."
            }
        }
    }

    pub fn wrap_user_message(self, message: &str) -> String {
        let mut wrapped = match self.explicit_intent {
            None => {
                format!("{UNIFIED_TURN_POLICY_PROMPT}\n\n[USER MESSAGE]\n{message}")
            }
            Some(_) => {
                format!(
                    "{}\nThe current user message is the only authority for this turn. Do not resume unfinished work from earlier turns unless this message explicitly requests it.\n\n[USER MESSAGE]\n{}",
                    self.prompt_contract(),
                    message
                )
            }
        };
        if self.answer_first {
            wrapped = wrapped.replace(
                "\n\n[USER MESSAGE]\n",
                "\nBefore any tool call, complete a concise user-facing answer to the requested question. Then perform only the authorized action.\n\n[USER MESSAGE]\n",
            );
        }
        wrapped
    }

    pub fn allows_mcp_tool(self, name: &str) -> bool {
        if name == "session_answer_save" {
            return self.answer_first;
        }
        if matches!(name, "session_reply_save" | "request_user_prompt") {
            return true;
        }
        match self.explicit_intent {
            Some(
                ProviderTurnIntent::Answer | ProviderTurnIntent::Plan | ProviderTurnIntent::Clarify,
            ) => false,
            Some(ProviderTurnIntent::Modify) | None => true,
            Some(ProviderTurnIntent::Inspect) => READ_ONLY_MCP_TOOLS.contains(&name),
        }
    }

    pub fn wrap_user_message_for_turn(self, message: &str, turn_nonce: &str) -> String {
        format!(
            "[ECKY TURN WRAPPER v1:{turn_nonce}]\n{}\n[ECKY END TURN WRAPPER v1:{turn_nonce}]",
            self.wrap_user_message(message)
        )
    }
}

pub fn unwrap_user_message(content: &str) -> Option<String> {
    if let Some((header, remainder)) = content.split_once('\n') {
        if let Some(nonce) = header
            .strip_prefix("[ECKY TURN WRAPPER v1:")
            .and_then(|value| value.strip_suffix(']'))
            .filter(|nonce| uuid::Uuid::parse_str(nonce).is_ok())
        {
            let ending = format!("\n[ECKY END TURN WRAPPER v1:{nonce}]");
            if let Some(body) = turn_wrapper_body(remainder, &ending) {
                if let Some(user_message) = unwrap_legacy_user_message(body) {
                    return Some(user_message);
                }
            }
        }
    }

    unwrap_legacy_user_message(content)
}

fn turn_wrapper_body<'a>(remainder: &'a str, ending: &str) -> Option<&'a str> {
    if let Some(body) = remainder.strip_suffix(ending) {
        return Some(body);
    }
    let ending_start = remainder.find(ending)?;
    let tail = &remainder[ending_start + ending.len()..];
    if tail.starts_with("\n\n[ATTACHMENT NOTES]\n") || tail.starts_with("\n\n[CAD ATTACHMENTS]\n") {
        Some(&remainder[..ending_start])
    } else {
        None
    }
}

fn unwrap_legacy_user_message(content: &str) -> Option<String> {
    let policies = [
        ProviderTurnPolicy::prompt_based(),
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer),
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Plan),
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Inspect),
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify),
        ProviderTurnPolicy::for_intent(ProviderTurnIntent::Clarify),
    ]
    .into_iter()
    .flat_map(|policy| [policy, ProviderTurnPolicy::routed(policy.intent(), true)])
    .collect::<Vec<_>>();
    // Older provider turns did not have a nonce. Decode only the exact wrappers
    // emitted by those versions; ordinary user-authored marker text stays intact.
    for policy in policies {
        let legacy = policy.wrap_user_message("");
        if let Some(user_message) = content.strip_prefix(&legacy) {
            return Some(user_message.to_string());
        }
    }
    None
}

const READ_ONLY_MCP_TOOLS: &[&str] = &[
    "health_check",
    "workspace_overview",
    "target_meta_get",
    "target_macro_get",
    "target_detail_get",
    "target_get",
    "thread_meta_get",
    "thread_messages_get",
    "thread_get",
    "artifact_manifest_get",
    "artifact_feature_graph_get",
    "compare_models",
    "ecky_ast_get",
    "ecky_ast_inspect",
    "ecky_ast_get_node",
    "ecky_ast_patch_validate",
    "semantic_manifest_get",
    "semantic_manifest_detail_get",
    "ecky_dependency_get",
    "ecky_selector_resolve",
    "ecky_constraints_validate",
    "get_structural_verification_summary",
    "get_model_screenshot",
    "component_search",
    "component_get",
    "freecad_library_search",
    "project_folder_status",
];

pub fn classify_turn_intent(message: &str) -> ProviderTurnIntent {
    if message.trim().is_empty() {
        ProviderTurnIntent::Answer
    } else {
        ProviderTurnIntent::Modify
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_turn_intent, unwrap_user_message, ProviderTurnIntent, ProviderTurnPolicy,
        UNIFIED_TURN_POLICY_PROMPT,
    };

    #[test]
    fn prompt_based_policy_provides_unified_turn_contract() {
        let policy = ProviderTurnPolicy::prompt_based();
        assert_eq!(policy.prompt_contract(), UNIFIED_TURN_POLICY_PROMPT);
        assert!(policy.allows_any_tool());
        assert!(policy.allows_project_writes());
        assert!(policy.allows_mcp_tool("macro_preview_render"));
    }

    #[test]
    fn explicit_policy_constrains_tools() {
        let answer = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer);
        assert!(!answer.allows_any_tool());
        assert!(!answer.allows_project_writes());

        let inspect = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Inspect);
        assert!(inspect.allows_mcp_tool("workspace_overview"));
        assert!(inspect.allows_mcp_tool("ecky_ast_inspect"));
        assert!(!inspect.allows_mcp_tool("ecky_ast_set_number"));
        assert!(!inspect.allows_mcp_tool("macro_preview_render"));
        assert!(!inspect.allows_project_writes());

        let modify = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Modify);
        assert!(modify.allows_mcp_tool("ecky_ast_set_number"));
        assert!(modify.allows_project_writes());

        for intent in [
            ProviderTurnIntent::Answer,
            ProviderTurnIntent::Plan,
            ProviderTurnIntent::Clarify,
        ] {
            let policy = ProviderTurnPolicy::for_intent(intent);
            assert!(!policy.allows_any_tool());
            assert!(!policy.allows_mcp_tool("workspace_overview"));
            assert!(!policy.allows_project_writes());
        }
    }

    #[test]
    fn mixed_modify_contract_requires_answer_before_mcp_tools() {
        let policy = ProviderTurnPolicy::routed(ProviderTurnIntent::Modify, true);
        assert!(policy.requires_answer_first());
        assert!(policy.allows_mcp_tool("ecky_ast_set_number"));
        assert!(policy
            .wrap_user_message("Explain and change this")
            .contains("Before any tool call"));
    }

    #[test]
    fn no_action_intents_can_save_user_facing_reply_without_project_tools() {
        for intent in [
            ProviderTurnIntent::Answer,
            ProviderTurnIntent::Plan,
            ProviderTurnIntent::Clarify,
        ] {
            let policy = ProviderTurnPolicy::for_intent(intent);
            assert!(policy.allows_mcp_tool("session_reply_save"));
            assert!(!policy.allows_mcp_tool("target_meta_get"));
            assert!(!policy.allows_mcp_tool("ecky_ast_set_number"));
        }
    }

    #[test]
    fn no_action_intents_can_wait_for_next_user_request() {
        for intent in [
            ProviderTurnIntent::Answer,
            ProviderTurnIntent::Plan,
            ProviderTurnIntent::Clarify,
        ] {
            let policy = ProviderTurnPolicy::for_intent(intent);
            assert!(policy.allows_mcp_tool("request_user_prompt"));
            assert!(!policy.allows_mcp_tool("thread_create"));
        }
    }

    #[test]
    fn turn_prompt_revokes_old_authority() {
        let policy = ProviderTurnPolicy::for_intent(ProviderTurnIntent::Answer);
        let prompt = policy.wrap_user_message("что происходит?");
        assert!(prompt.contains("Intent: ANSWER"));
        assert!(prompt.contains("only authority for this turn"));
        assert!(prompt.ends_with("что происходит?"));
    }

    #[test]
    fn prompt_based_turn_wraps_user_message() {
        let policy = ProviderTurnPolicy::prompt_based();
        let prompt = policy.wrap_user_message("любой запрос");
        assert!(prompt.contains(UNIFIED_TURN_POLICY_PROMPT));
        assert!(prompt.contains("[USER MESSAGE]"));
        assert!(prompt.ends_with("любой запрос"));
    }

    #[test]
    fn provider_user_projection_decodes_only_exact_turn_wrapper() {
        let original = "Keep [USER MESSAGE] in my authored text.";
        let wrapped = ProviderTurnPolicy::prompt_based()
            .wrap_user_message_for_turn(original, "00000000-0000-4000-8000-000000000001");
        assert_eq!(unwrap_user_message(&wrapped).as_deref(), Some(original));
        assert_eq!(
            unwrap_user_message(&wrapped.replace(
                "[ECKY END TURN WRAPPER v1:00000000-0000-4000-8000-000000000001]",
                "[ECKY END TURN WRAPPER v1:00000000-0000-4000-8000-000000000002]",
            )),
            None
        );
        assert_eq!(
            unwrap_user_message("Please keep [USER MESSAGE] exactly as typed."),
            None
        );
    }

    #[test]
    fn every_explicit_and_answer_first_policy_round_trips_original_user_text() {
        let original = "Explain, then change the project. [USER MESSAGE]";
        for intent in [
            ProviderTurnIntent::Answer,
            ProviderTurnIntent::Plan,
            ProviderTurnIntent::Inspect,
            ProviderTurnIntent::Modify,
            ProviderTurnIntent::Clarify,
        ] {
            for answer_first in [false, true] {
                let wrapped = ProviderTurnPolicy::routed(intent, answer_first)
                    .wrap_user_message_for_turn(original, "00000000-0000-4000-8000-000000000001");
                assert_eq!(unwrap_user_message(&wrapped).as_deref(), Some(original));
            }
        }
    }

    #[test]
    fn empty_message_classifies_as_answer() {
        assert_eq!(classify_turn_intent("   "), ProviderTurnIntent::Answer);
    }
}
