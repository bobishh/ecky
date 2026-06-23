# Tasks: AST Visual Construction Blocks

## 1. Acceptance fixture + outer red

- [ ] 1.1 Add phone-case fixture `src-tauri/tests/fixtures/cad/blocks/phone_case.ecky`: params, derived dimensions, structural solids, corner bumpers, pocket cutter, rim cutter, camera cutters, port cutter, side button cutters (`(translate .. (rotate 0 -90 0 (extrude (rounded-rect ..) ..)))`), final `difference`, verify clauses.
- [ ] 1.2 Red integration test (Rust): block-tree projection of the fixture yields nested groups, roles, spans, evaluated derived values — fails because the command does not exist.

## 2. Block tree projection (backend)

- [ ] 2.1 Red unit: `let*` bindings inside `part` become addressable nodes with byte spans and ids `model/part:<name>/let/bindings/<binding>` (extend `commands/macro_ast.rs` parse walk).
- [ ] 2.2 Green: descend into `part` `let*` forms, emit binding nodes with spans from `Parser::parse_without_lowering`.
- [ ] 2.3 Red unit: role inference — suffix rules (`-solid|-body|-bumper` solid; `-cutter|-cut|-opening|-hole` cutter), head rules (`rounded-rect` profile, `difference` boolean, scalar binding derived-value), usage rule (non-first `difference` operand → cutter through reference chain), usage-beats-name conflict marker.
- [ ] 2.4 Green: implement role inference with `role` + `roleSource` on every node.
- [ ] 2.5 Red unit: derived value nodes carry `formula` (exact source slice), `evaluatedValue` from compiled `CoreProgram` with current params, and `references` edges to source bindings; non-static bindings report runtime.
- [ ] 2.6 Green: evaluation + reference edges from core IR `Reference` nodes.
- [ ] 2.7 Red unit: geometry summary flattens transform chains — the button-cutter expression summarizes to translate vector, rotate angles, extrude depth, profile dims.
- [ ] 2.8 Green: summary struct for translate/rotate/scale over extrude/cylinder/profile heads.
- [ ] 2.9 Red unit: grouping — section comments start named groups; contiguous same-role runs group by role; final boolean chain is its own group.
- [ ] 2.10 Green: grouping pass over the binding list.
- [ ] 2.11 Red unit: verify clauses get node ids and `anchorNodeId` resolved from selectors naming a binding/part; unresolvable anchors attach to the part.
- [ ] 2.12 Green: verification attachment.
- [ ] 2.13 Expose `macro_ast_block_tree` Tauri command (`#[serde(rename_all = "camelCase")]`, specta), regenerate bindings; outer test 1.2 green; `cargo check` + `cargo test` green.

## 3. Block view (frontend)

- [ ] 3.1 Red Playwright: opening the phone-case model in the block view shows collapsible groups; expanding "cutters" reveals badged cutter bindings with summaries; fails, view absent.
- [ ] 3.2 Red unit (`macroAstBlocks.test.ts`): projection of a block-tree payload into renderable rows — collapse state, role badges, derived-value line `name = value = formula`.
- [ ] 3.3 Green: `MacroAstBlocks.svelte` collapsible tree beside `MacroSourcePane`, reusing selection/id contract from `macroAstMap.ts`; Tactical Midnight, square borders, `overflow: hidden`.
- [ ] 3.4 Red unit: clicking a reference in a derived-value formula selects the referenced binding node.
- [ ] 3.5 Green: reference navigation.
- [ ] 3.6 Red Playwright: block → source selection highlights the byte span in the source pane; placing the cursor in source selects and expands the containing block.
- [ ] 3.7 Green: bidirectional span-based sync.
- [ ] 3.8 Playwright 3.1 green; `npm run test:unit` green.

## 4. Debug previews

- [ ] 4.1 Red integration: preview request `shape-only` for a cutter binding renders just that shape via the existing preview pipeline, tagged debug (never export).
- [ ] 4.2 Green: backend synthesizes preview variant with part result = selected binding.
- [ ] 4.3 Red: `accumulated` preview truncates the `let*` after the selected binding and renders construction state.
- [ ] 4.4 Green.
- [ ] 4.5 Red: `all-cutters` renders every cutter-role shape as debug overlay over the base solid.
- [ ] 4.6 Green.
- [ ] 4.7 Playwright: selecting `right-power-button-through-wall-cutter` and choosing each preview mode updates the viewport; export artifacts contain no debug geometry.

## 5. Verification display + parameter editing

- [ ] 5.1 Red Playwright: verify results render at their anchor blocks (pass/fail/error + raw message), not only in a global log.
- [ ] 5.2 Green: map runtime verify results onto anchor node ids.
- [ ] 5.3 Red Playwright: editing a parameter from the block view goes through the existing AST-patch flow; derived values and preview update; rejection shows raw backend error at the param node.
- [ ] 5.4 Green: wire param controls to the existing patch path.

## 6. Acceptance + hygiene

- [ ] 6.1 Walk the acceptance criteria on the phone-case fixture: collapse sections, expand a feature, distinguish solids/cutters, inspect a cutter's expression + preview, read the final difference structure, round-trip selection both ways.
- [ ] 6.2 Full suites green: `cargo test`, `npm run test:unit`, `npm run test:e2e`.
- [ ] 6.3 `openspec validate ast-visual-blocks` passes.
