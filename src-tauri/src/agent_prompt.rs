//! Single-source agent language reference (OpenSpec `agent-prompt-single-source`).
//!
//! Assembles the language body shared by the in-app MCP agent and an API-mode
//! agent that has no MCP — it can only emit `.ecky` source and read the compiler
//! diagnostic on a failed request. The body is therefore self-contained:
//!
//!   API operating contract  +  concise `.ecky` language guide  +  op catalogue
//!
//! The op catalogue is rendered from `ecky_language_surface::supported_surface_reference`
//! (derived from the real op set), so adding/removing an op updates the prompt
//! automatically and cannot drift. Both MCP (`ecky://guides/technical-system-prompt`)
//! and API mode call `agent_language_reference` so their language rules are identical.

use crate::ecky_language_surface::supported_surface_reference;
use crate::models::GeometryBackend;

/// Upper bound for the assembled prompt. ~8K tokens ≈ 32K chars (see the change's
/// design.md). Overflow is a signal to tighten the body, not to raise the limit.
pub const AGENT_PROMPT_CHAR_CEILING: usize = 32_000;

/// Operating rules for a tool-less API-mode agent. Prepended to the language body.
pub const API_OPERATING_CONTRACT: &str = "\
# Ecky authoring — operating contract

You author `.ecky` source only. You have no tools and no documents to fetch:
everything you need to write valid source is in this prompt.

- Units: all lengths are millimetres, all angles are degrees. Bare numbers are
  already in these units; suffixes (`mm`/`cm`/`in`, `deg`/`rad`) only convert
  into them. Ecky does not type-check dimensions — that discipline is yours.
- Output a single `(model ...)` program. Keep `params`, geometry, and any
  `verify` clauses consistent.
- On a failed request you receive the compiler diagnostic. Treat it as
  authoritative: fix the named cause and re-emit. A diagnostic naming an op as
  unsupported on the active backend (e.g. native-only `:created-by`, or
  `:to-radius` rejected by build123d) means switch the approach or the backend,
  not retry verbatim.
- Respect the per-op backend support listed in the op catalogue below. Prefer
  geometry that renders on the active backend.
";

/// The full self-contained language reference for `backend`.
pub fn agent_language_reference(backend: GeometryBackend) -> String {
    format!(
        "{contract}\n{guide}\n\n{catalogue}",
        contract = API_OPERATING_CONTRACT,
        guide = crate::commands::generation::ecky_source_guide_text(),
        catalogue = op_catalogue(backend),
    )
}

/// Op catalogue as documentation-by-example: one worked `.ecky` snippet per form
/// with a short trailing comment. Injected from the surface reference (the
/// `example` + `description` fields), never hand-written, so it cannot drift.
/// LLMs author far more reliably from a commented example than from prose, and
/// the example line is usually terser than a signature + sentence.
fn op_catalogue(backend: GeometryBackend) -> String {
    let reference = supported_surface_reference(backend);
    let mut out = String::from(
        "# Op catalogue — one worked example per form\n\
         Every snippet below renders on the active backend. Comments note what each form does;\n\
         a `[...]` note marks a backend restriction.\n\n```scheme\n",
    );
    for entry in &reference.entries {
        // Prefer the real example; fall back to the signature shape if absent.
        let code = if entry.example.trim().is_empty() {
            entry.signature.as_str()
        } else {
            entry.example.as_str()
        };
        out.push_str(code);
        let mut comment = first_line(&entry.description).to_string();
        // Catalogue is already backend-filtered, so surface support only when restricted.
        if is_restricted_support(&entry.backend_support) {
            comment.push_str(" [");
            comment.push_str(&entry.backend_support);
            comment.push(']');
        }
        if !comment.is_empty() {
            out.push_str("  ; ");
            out.push_str(&comment);
        }
        out.push('\n');
    }
    out.push_str("```\n");
    out
}

fn is_restricted_support(support: &str) -> bool {
    support.contains("only") || support.contains("rejected")
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backends() -> [GeometryBackend; 3] {
        [
            GeometryBackend::EckyRust,
            GeometryBackend::Build123d,
            GeometryBackend::Freecad,
        ]
    }

    // Self-containment: an API agent has no MCP, so the prompt must not depend on
    // tool calls and must carry no image markup.
    #[test]
    fn agent_prompt_is_self_contained() {
        for backend in backends() {
            let prompt = agent_language_reference(backend);
            assert!(
                !prompt.contains("!["),
                "{backend:?} prompt contains image markup"
            );
            assert!(
                !prompt.contains("mcp_"),
                "{backend:?} prompt references an MCP tool"
            );
            assert!(
                prompt.contains("operating contract"),
                "{backend:?} prompt missing the API contract"
            );
        }
    }

    // Drift guard: every op in the surface reference appears in the prompt's
    // catalogue, so a newly added op shows up without editing the prompt.
    #[test]
    fn agent_prompt_op_catalogue_covers_surface_reference() {
        for backend in backends() {
            let prompt = agent_language_reference(backend);
            for entry in supported_surface_reference(backend).entries {
                assert!(
                    prompt.contains(&entry.name),
                    "{backend:?} prompt is missing op `{}` from the surface reference",
                    entry.name
                );
            }
        }
    }

    // NOTE: the assertion that a specific newly-added op (torus, slot, thread…)
    // surfaces in the prompt belongs with the op's own change — it depends on the
    // op being registered in `ecky_language_surface` (and backed by `cad::MODULE`).
    // The builder's own contract is `agent_prompt_op_catalogue_covers_surface_reference`
    // above: whatever the surface reference exposes, the prompt lists.

    // Budget guard: the assembled prompt stays under the configured ceiling.
    #[test]
    fn agent_prompt_stays_within_budget() {
        for backend in backends() {
            let prompt = agent_language_reference(backend);
            assert!(
                prompt.len() <= AGENT_PROMPT_CHAR_CEILING,
                "{backend:?} prompt is {} chars, over the {AGENT_PROMPT_CHAR_CEILING} ceiling",
                prompt.len()
            );
        }
    }
}
