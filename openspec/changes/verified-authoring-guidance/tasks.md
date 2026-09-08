# Tasks

- [x] Add failing focused integration coverage for catalogue compile/native-plan
  examples and bounded guidance claims.
- [x] Correct `clip-plane`, `clip-box`, and Bézier catalogue entries.
- [x] Add canonical and MCP authoring guidance for evidence and parameter rules.
- [x] Regenerate committed prompt artifacts from canonical sources.
- [x] Run focused Rust tests and record exact red/green evidence.

## Evidence — 2026-09-08

- Red contract run caught missing `all three bounds` guidance; native examples
  already exposed that bare `positive` plans in a simple literal context.
- Named local `positive` reproduces the real native planning error exactly:
  `Direct OCCT adapter could not resolve local \`positive\`.`
- Final `cargo test --test agent_authoring_contract -- --nocapture`: 2 passed.

- `npm run generate:docs` completed; `npm run check:book-source` passed.
- Five content projection/source tests passed. Prose is edited in the corpus,
  then projected into the runtime include and generated prompts.
