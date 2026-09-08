//! Single-source agent language reference (OpenSpec `agent-prompt-single-source`).
//!
//! Assembles the language body shared by the in-app MCP agent and an API-mode
//! agent that has no MCP — it can only emit `.ecky` source and read the compiler
//! diagnostic on a failed request. The body is therefore self-contained:
//!
//!   Mode-specific operating contract + shared `.ecky` guide + op catalogue
//!
//! The op catalogue is rendered from `ecky_language_surface::supported_surface_reference`
//! (derived from the real op set), so adding/removing an op updates the prompt
//! automatically and cannot drift. MCP and API share the language body while
//! retaining their own operating instructions.

use crate::contracts::GeometryBackend;
use crate::ecky_language_surface::supported_surface_reference;

const CANONICAL_AGENT_REFERENCE: &str = include_str!("../../public/docs/ecky-agent-reference.md");

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
  unsupported on the active backend (e.g. native-only `:created-by` rejected by
  FreeCAD interop) means switch the approach or the backend,
  not retry verbatim.
- Respect the per-op backend support listed in the op catalogue below. Prefer
  geometry that renders on the active backend.
";

/// Tool-aware rules for MCP callers; language content remains shared with API mode.
const MCP_OPERATING_CONTRACT: &str = "\
# Ecky authoring — MCP operating contract

Use Ecky MCP tools to inspect, validate, render, and verify `.ecky` models.

- Inspect the current target and its `sourcePath`, parameters, and backend.
  Edit a bound source file directly; wait for its watcher render to finish.
  Use preview tools for unbound targets or guarded AST patches.
- Constraint validation is not render proof. A saved file or successful compile
  does not prove that geometry rendered or that active parameter values changed.
- Read the completed render result for the exact edited version, then call
  `verify_generated_model` with that version's `messageId`. Report completion
  only from matching render and verification evidence; inspect viewport evidence
  before making visual claims.
- Preserve raw diagnostics. Isolate the named failing expression before broader
  edits; do not repeatedly rewrite unrelated geometry or retry identical failures.
- Native catalogue examples are tested through compile and native planning. Keep
  every required literal or named binding, including all `clip-box` ranges; do
  not replace a compiler error with Python, STL, or hardcoded controls. For
  `clip-plane`, prefer quoted text for `:keep` (\"positive\" or \"negative\");
  a known bare literal may work, while an unresolved local `positive` can fail
  name resolution.
- The `geometryBackend=mesh` label is a legacy native-hybrid setting; it does not
  prove mesh execution. Use exact artifact truth (`analyticBrep` versus faceted
  mesh) for representation claims.
- Native Bézier paths use a fixed 16 samples per cubic; this is an approximation.
  Topology checks such as zero non-manifold edges
  or one component do not prove cross-section, shape intent, or support-free
  printing.
- Preserve parameters and report the exact current version. Treat visual or
  mechanical hypotheses as hypotheses until measured; inspect matching screenshot
  and artifact evidence before claiming intent.
";

/// The full self-contained language reference for a tool-less API caller.
pub fn agent_language_reference(backend: GeometryBackend) -> String {
    language_reference(backend, API_OPERATING_CONTRACT)
}

/// The same language reference with instructions for a tool-equipped MCP caller.
pub fn mcp_language_reference(backend: GeometryBackend) -> String {
    language_reference(backend, MCP_OPERATING_CONTRACT)
}

fn language_reference(backend: GeometryBackend, contract: &str) -> String {
    let backend_label = match backend {
        GeometryBackend::Build123d => "mesh (legacy setting migrated to Ecky Native)",
        GeometryBackend::Freecad => "freecad",
        GeometryBackend::EckyRust => "mesh (legacy native-hybrid label; inspect artifact truth)",
    };
    format!(
        "{contract}\nTarget geometryBackend: `{backend_label}`.\n\n{guide}\n\n{catalogue}",
        guide = canonical_agent_reference(),
        catalogue = op_catalogue(backend),
    )
}

/// Agent-facing language body projected separately from the canonical corpus.
/// Human tutorial/reference routes never need to strip prompt-only policy.
pub fn canonical_agent_reference() -> &'static str {
    CANONICAL_AGENT_REFERENCE.trim()
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
        if entry.kind == "componentPlacementForm" {
            continue;
        }
        // Prefer the real example; fall back to the signature shape if absent.
        let code = if entry.example.trim().is_empty() {
            entry.signature.as_str()
        } else {
            entry.example.as_str()
        };
        // Most examples already name their form. Avoid repeating a catalogue
        // heading unless the executable snippet cannot identify the entry.
        if !code.contains(&entry.name) {
            out.push_str("; ");
            out.push_str(&entry.name);
            out.push('\n');
        }
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

    #[test]
    fn mcp_prompt_keeps_tools_and_requires_render_evidence() {
        for backend in backends() {
            let prompt = mcp_language_reference(backend);
            assert!(!prompt.contains("You have no tools"));
            assert!(prompt.contains("sourcePath"));
            assert!(prompt.contains("Constraint validation is not render proof"));
            assert!(prompt.contains("verify_generated_model"));
            assert!(prompt.contains(canonical_agent_reference()));
            assert!(prompt.len() <= AGENT_PROMPT_CHAR_CEILING);
            assert!(agent_language_reference(backend).contains(API_OPERATING_CONTRACT));
        }
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

    #[test]
    fn agent_prompt_body_is_projected_verbatim_from_the_canonical_book() {
        let body = canonical_agent_reference();
        assert!(body.contains("`mesh` and `polyhedron`"));
        assert!(body.contains("`protrude`"));
        assert!(!body.contains("`heightfield`"));

        for backend in backends() {
            assert!(agent_language_reference(backend).contains(body));
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
