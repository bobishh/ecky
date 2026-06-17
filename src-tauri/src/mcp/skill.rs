//! Rendering for the repo-owned Ecky MCP skill.
//!
//! The tool catalog is the drift-prone part of the skill, so it is generated
//! from the server's own `tool_definitions_with_ast_enabled` rather than
//! hand-maintained. `render_tools_markdown` is a pure function over that catalog
//! so it can be unit-tested without a running server. See `bin/export_mcp_skill`.

use serde_json::Value;

/// Argument names declared by a tool's `inputSchema`, paired with whether the
/// argument is required. Order follows the schema's `properties` map.
fn tool_arg_names(input_schema: &Value) -> Vec<(String, bool)> {
    let required: Vec<&str> = input_schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    input_schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .keys()
                .map(|name| (name.clone(), required.contains(&name.as_str())))
                .collect()
        })
        .unwrap_or_default()
}

fn render_args_line(input_schema: &Value) -> String {
    let args = tool_arg_names(input_schema);
    if args.is_empty() {
        return "Arguments: none".to_string();
    }
    let rendered: Vec<String> = args
        .into_iter()
        .map(|(name, required)| {
            if required {
                format!("`{name}` (required)")
            } else {
                format!("`{name}`")
            }
        })
        .collect();
    format!("Arguments: {}", rendered.join(", "))
}

/// Render the full tool catalog as the skill's `reference/tools.md`.
pub fn render_tools_markdown(tools: &[Value]) -> String {
    let mut out = String::new();
    out.push_str("# Ecky MCP Tools\n\n");
    out.push_str(
        "_Generated from the live MCP tool catalog by `cargo run --bin export_mcp_skill` \
         (`npm run generate:skill`). Do not edit by hand._\n",
    );

    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let empty_schema = Value::Object(serde_json::Map::new());
        let input_schema = tool.get("inputSchema").unwrap_or(&empty_schema);

        out.push_str(&format!("\n## {name}\n\n"));
        if !description.is_empty() {
            out.push_str(description);
            out.push_str("\n\n");
        }
        out.push_str(&render_args_line(input_schema));
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Vec<Value> {
        vec![
            json!({
                "name": "health_check",
                "description": "Confirm server is alive.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "macro_preview_render",
                "description": "Preview an .ecky source.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "agentId": {}, "threadId": {}, "source": {} },
                    "required": ["agentId", "source"]
                }
            }),
        ]
    }

    #[test]
    fn renders_header_and_generation_notice() {
        let md = render_tools_markdown(&fixture());
        assert!(md.starts_with("# Ecky MCP Tools"));
        assert!(md.contains("Do not edit by hand"));
    }

    #[test]
    fn renders_each_tool_name_and_description() {
        let md = render_tools_markdown(&fixture());
        assert!(md.contains("## health_check"));
        assert!(md.contains("Confirm server is alive."));
        assert!(md.contains("## macro_preview_render"));
        assert!(md.contains("Preview an .ecky source."));
    }

    #[test]
    fn renders_no_args_as_none() {
        let md = render_tools_markdown(&fixture());
        assert!(md.contains("Arguments: none"));
    }

    #[test]
    fn renders_arg_names_and_marks_required() {
        let md = render_tools_markdown(&fixture());
        assert!(md.contains("`agentId` (required)"));
        assert!(md.contains("`source` (required)"));
        assert!(md.contains("`threadId`"));
        // threadId is optional → no (required) suffix on it
        assert!(!md.contains("`threadId` (required)"));
    }

    #[test]
    fn skips_tools_without_a_name() {
        let tools = vec![json!({ "description": "nameless" })];
        let md = render_tools_markdown(&tools);
        assert!(!md.contains("nameless"));
    }
}
