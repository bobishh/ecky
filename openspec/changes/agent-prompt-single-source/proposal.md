# Proposal: Single-Source Language Reference → Three Artifacts

## Intent

Today the Ecky language is described to humans and to agents in **separate,
hand-maintained places that silently drift**:

- the **book** (`public/docs/ecky-ir.md`) — rich prose + rendered images, humans;
- the **MCP guides** (`ecky://guides/*`, hand-written Rust text functions) — the
  in-app MCP agent, which can read them incrementally;
- there is **no API-mode system prompt** at all — yet an agent driving Ecky over
  the API has **no MCP**: it can only emit `.ecky` source and, on a failed
  request, read the compiler diagnostic to retry.

Only the op catalogue auto-syncs (the `surface-reference` resource is derived
from the real op set). Everything else is copied by hand.

This change establishes **one source of truth for the language reference** and
emits **three artifacts** from it:

1. **Book** — full prose + images, for humans (the existing EPUB/HTML build).
2. **MCP guides** — for the in-app MCP agent.
3. **API system prompt** — a **compressed, self-contained** projection for an
   API-mode LLM: no images, drier language, the full language reference inline
   (the agent cannot look anything up), the auto op catalogue, and the
   code → diagnostic → retry operating contract.

## Why the API prompt is special

The API agent has no tools and no incremental guides. So its system prompt must
be **complete** (every grammar rule, op, and common pitfall it needs to author
correctly is in the prompt) **and compact** (it has a context budget). That is a
distillation, not a copy of the book: strip images, drop narrative warmth, keep
code + rules + a compact op table, and teach self-correction from compiler
diagnostics.

## Scope

- Designate `public/docs/ecky-ir.md` + the auto `surface-reference` as the single
  source for language content.
- Add a generator (extend `build:book`) that emits, from that source:
  - the book (unchanged), and
  - an **API system prompt** artifact (compressed: images stripped, prose
    distilled, op catalogue injected from `surface-reference`, plus the
    API-mode operating contract).
- Route the MCP `technical-system-prompt` / language guides and the API-mode
  prompt through **one builder** so the agent-facing content never forks.
- Add a **drift check**: the op set named in the generated prompt equals the
  `surface-reference` op set equals the book appendix; CI fails on divergence.

## Out of scope

- The in-app generation JSON output contract (`TECHNICAL_SYSTEM_PROMPT` shape)
  beyond pointing it at the shared language section.
- New agent runtime/transport for API mode (assumed to exist or land separately).
