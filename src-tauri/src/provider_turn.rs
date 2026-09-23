use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderTurnIntent {
    Answer,
    Inspect,
    Modify,
    Clarify,
}

impl ProviderTurnIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Inspect => "inspect",
            Self::Modify => "modify",
            Self::Clarify => "clarify",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "answer" => Some(Self::Answer),
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
- CLARIFY: If the request is ambiguous, contradictory, or lacks necessary dimensions/requirements, ask one concise clarifying question without editing files.\n\
The current user message is the only authority for this turn. Do not resume unfinished work from earlier turns unless this message explicitly requests it.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTurnPolicy {
    explicit_intent: Option<ProviderTurnIntent>,
}

impl ProviderTurnPolicy {
    pub fn prompt_based() -> Self {
        Self {
            explicit_intent: None,
        }
    }

    pub fn for_intent(intent: ProviderTurnIntent) -> Self {
        Self {
            explicit_intent: Some(intent),
        }
    }

    pub fn unified_prompt_contract() -> &'static str {
        UNIFIED_TURN_POLICY_PROMPT
    }

    pub fn is_prompt_based(self) -> bool {
        self.explicit_intent.is_none()
    }

    pub fn allows_any_tool(self) -> bool {
        match self.explicit_intent {
            Some(ProviderTurnIntent::Answer) => false,
            _ => true,
        }
    }

    pub fn allows_project_writes(self) -> bool {
        match self.explicit_intent {
            Some(ProviderTurnIntent::Modify) | None => true,
            _ => false,
        }
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
            Some(ProviderTurnIntent::Inspect) => {
                "[TURN POLICY]\nIntent: INSPECT\nUse only advertised read-only tools. Do not edit project files or resume unfinished authoring. Return the answer after bounded inspection."
            }
            Some(ProviderTurnIntent::Modify) => {
                "[TURN POLICY]\nIntent: MODIFY\nPerform the change requested in the current user message. Inspect, edit, preview, and verify."
            }
            Some(ProviderTurnIntent::Clarify) => {
                "[TURN POLICY]\nIntent: CLARIFY\nAnswer the user or ask a clarifying question. If enough details are present, proceed with CAD modifications using MCP tools."
            }
        }
    }

    pub fn wrap_user_message(self, message: &str) -> String {
        match self.explicit_intent {
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
        }
    }

    pub fn allows_mcp_tool(self, name: &str) -> bool {
        match self.explicit_intent {
            Some(ProviderTurnIntent::Answer) => false,
            Some(ProviderTurnIntent::Modify) | Some(ProviderTurnIntent::Clarify) | None => true,
            Some(ProviderTurnIntent::Inspect) => READ_ONLY_MCP_TOOLS.contains(&name),
        }
    }
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
        classify_turn_intent, ProviderTurnIntent, ProviderTurnPolicy, UNIFIED_TURN_POLICY_PROMPT,
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
    fn empty_message_classifies_as_answer() {
        assert_eq!(classify_turn_intent("   "), ProviderTurnIntent::Answer);
    }
}
