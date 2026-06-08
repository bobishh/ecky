//! CAD transpile — a thin LLM translate over the shared Ecky language reference.
//!
//! Transpile is not an engine. It assembles the same self-contained Ecky system
//! prompt the agent already uses (`agent_prompt::agent_language_reference`) plus a
//! fixed translate instruction, sends the foreign CAD source as the user message
//! through the existing OpenAI-compatible client, and returns `.ecky`. The
//! compile + `verify` gate (elsewhere) decides trust; this module only builds the
//! request and (in the binary) performs the call.

use crate::agent_prompt::agent_language_reference;
use crate::models::GeometryBackend;

/// Fixed translate instruction prepended to the foreign source in the user
/// message. It carries the *semantic* ask the deterministic transpiler could not
/// do (parametrize, loop-ify) plus a portability rule learned from real output:
/// the facet-count argument on `cylinder`/`circle` is a tessellation hint and is
/// ignored on non-native backends, so a true polygonal prism must use
/// `regular-polygon` + `extrude`.
pub const TRANSLATE_PREAMBLE: &str = "\
Translate the CAD source below into ONE parametric Ecky `(model ...)` program.

- Infer meaningful numeric parameters (sizes, counts, repeats) into a `(params ...)`
  block; do not copy dead numbers. Derive dependent dimensions as expressions.
- Express repeated features as loops (`repeat-union` / `for-union`), never as N
  hand-copied translated solids.
- Portability: the facet-count argument on `cylinder`/`circle` is a tessellation
  hint only and is IGNORED on non-native backends (it renders round). For a true
  polygonal prism (e.g. a hex bolt head) use `(extrude (regular-polygon SIDES
  circumradius) height)`, never a faceted cylinder.
- Add `(verify ...)` clauses for the invariants that must hold (at least a single
  watertight solid: `stl connected-component-count` = 1, `stl
  non-manifold-edge-count` = 0).
- Output ONLY Ecky source — no prose, no code fences.

CAD source:
";

/// Build the `(system, user)` message pair for a transpile request. `system` is
/// the shared, drift-free Ecky language reference for `backend`; `user` is the
/// fixed translate instruction followed by the source verbatim.
pub fn build_transpile_messages(source: &str, backend: GeometryBackend) -> (String, String) {
    let system = agent_language_reference(backend);
    let user = format!("{TRANSLATE_PREAMBLE}\n{source}");
    (system, user)
}

/// Strip a single Markdown code fence if the model wrapped its reply in one,
/// tolerating an optional language tag (` ```scheme `) and a trailing fence.
pub fn strip_code_fence(reply: &str) -> String {
    let trimmed = reply.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    // Drop the rest of the opening fence line (an optional language tag).
    let body = rest.splitn(2, '\n').nth(1).unwrap_or("");
    body.trim()
        .strip_suffix("```")
        .unwrap_or(body)
        .trim()
        .to_string()
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
    fn system_is_the_shared_language_reference_and_user_carries_the_source() {
        let source = "// foreign\ncube([1,2,3]);";
        for backend in backends() {
            let (system, user) = build_transpile_messages(source, backend);
            assert_eq!(
                system,
                agent_language_reference(backend),
                "{backend:?} system prompt must be the shared reference verbatim"
            );
            assert!(user.contains(source), "{backend:?} user must contain the source");
            assert!(
                user.starts_with("Translate the CAD source"),
                "{backend:?} user must lead with the translate instruction"
            );
        }
    }

    #[test]
    fn preamble_carries_the_semantic_ask_and_portability_rule() {
        let (_system, user) = build_transpile_messages("x", GeometryBackend::Build123d);
        for needle in [
            "(params",
            "repeat-union",
            "regular-polygon",
            "(verify",
            "ONLY Ecky",
        ] {
            assert!(user.contains(needle), "preamble missing `{needle}`");
        }
    }

    #[test]
    fn strip_code_fence_handles_fenced_and_bare_replies() {
        let bare = "(model (part p (box 1 1 1)))";
        assert_eq!(strip_code_fence(bare), bare);

        let fenced = "```scheme\n(model (part p (box 1 1 1)))\n```";
        assert_eq!(strip_code_fence(fenced), "(model (part p (box 1 1 1)))");

        let fenced_no_lang = "```\n(model)\n```";
        assert_eq!(strip_code_fence(fenced_no_lang), "(model)");

        let with_prose_then_fence = "  ```ecky\n(a)\n(b)\n```  ";
        assert_eq!(strip_code_fence(with_prose_then_fence), "(a)\n(b)");
    }
}
