# Ecky MCP Tools

_Generated from the live MCP tool catalog by `cargo run --bin export_mcp_skill` (`npm run generate:skill`). Do not edit by hand._

## health_check

Confirm server is alive and can reach storage/runtime.

Arguments: none

## workspace_overview

Fast entrypoint: resolve the default editable target, list recent threads, and report any conflicting lease.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`

## freecad_library_search

Search configured local FreeCAD-library folders for reusable FCStd/STEP/STL parts. Architecture folders are excluded unless includeArchitecture is true.

Arguments: `includeArchitecture`, `limit`, `query` (required), `roots`

## project_folder_export

Mirror a thread's active macro into a plain folder (<projectsRoot>/<slug>/model.ecky + ecky-project.json) so external editors and file-skill agents can author source directly. Re-export refreshes a stale folder.

Arguments: `messageId`, `slug`, `threadId`

## project_folder_status

Read-only sync classification of a project folder: clean | fileChanged | threadAdvanced | conflict | missing.

Arguments: `slug` (required)

## project_folder_apply

Apply an externally edited model.ecky back onto its bound thread: compile check, preview render, commit as a new version, rebase the folder manifest. Refuses stale (threadAdvanced) folders; conflict needs force=true.

Arguments: `force`, `slug` (required), `title`, `versionName`

## component_extract

Lift an existing part subtree into a closed, copy-inline `define-component` snippet. Referenced model params become the signature (metadata preserved); scalar outer let bindings become plain defaults; other free references are reported as blockers. Optionally saves the component into the component library.

Arguments: `description`, `messageId`, `name`, `partKey` (required), `save`, `source` (required), `tags`, `threadId`

## component_search

Search the component library by compact header (name, one-liner, param keys, tags). Header-only: never returns component bodies; use component_get for source.

Arguments: `limit`, `query`

## component_get

Fetch one library component by name: full copy-inline `define-component` source plus its header.

Arguments: `name` (required)

## freecad_library_import

Import one FreeCAD-library search result into an Ecky thread. Materializes runtime artifacts, creates a visible imported model version, and returns threadId/messageId plus artifactBundle/modelManifest.

Arguments: `item` (required), `threadId`, `title`

## session_log_in

Notify the workspace that an agent has joined. threadId/messageId are optional: pass them only to claim an initial target; omit them for a targetless session. A session may later work on other threads by calling thread_borrow, passing explicit threadId/messageId to tools, or calling thread_create. If another live agent already owns an explicit thread target, the call fails unless stealThread is true.

Arguments: `agentLabel` (required), `messageId`, `modelId`, `stealThread`, `threadId`

## session_log_out

Notify the workspace that an agent is leaving.

Arguments: `agentLabel` (required)

## resume_session

Resume a previous agent session by retrieving the last known context.

Arguments: `agentLabel` (required)

## thread_list

Lightweight browsing of available work targets. Includes queued/pending counts, pendingConfirm, and latestPendingMessageId so agents can sweep inbox threads without loading full histories.

Arguments: none

## thread_create

Create a new blank thread and borrow it as this MCP session's current target. Use this for a new design before calling macro_preview_render. Authoring language/backend belong to the model version or session config, not the thread.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `title`

## thread_borrow

Borrow an existing thread as this MCP session's current target without logging out/in. Use this after thread_list/thread_get when choosing or switching existing work. Pass messageId to target a specific version; otherwise pass threadId for the latest/default target.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `stealThread`, `threadId`

## thread_meta_get

Fetch thread metadata without messages. Includes pendingConfirm and latestPendingMessageId for inbox/claim workflows.

Arguments: `threadId` (required)

## thread_messages_get

Fetch a slice of compact messages from a thread.

Arguments: `before`, `limit`, `roles`, `threadId` (required)

## thread_get

Fetch a full thread with versions and runtime metadata. Expensive; prefer thread_meta_get/thread_messages_get.

Arguments: `threadId` (required)

## agent_identity_set

Set sticky agent/model labels for this MCP session.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`

## ui_dispatch

Trigger a UI action in the Ecky frontend to show the user what you are doing (e.g. open the parameters window, highlight a specific slider).

Arguments: `action` (required), `target` (required), `value`

## target_meta_get

Fetch a lightweight summary of the current editable target. Preferred default read step after workspace_overview. Includes scenePacket plus artifact routing flags hasArtifactBundle, hasRuntimeManifest, edgeTargetCount, faceTargetCount, exportFormats, hasStepExport, and stepExportPath; call artifact_manifest_get for full JSON.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`

## target_macro_get

Fetch active editable source metadata plus a 1-based line window, authoringContext, and artifactDigest. Pass startLine/endLine for a specific range. Prefer macro_buffer_get for edits.

Arguments: `agentLabel`, `endLine`, `llmModelId`, `llmModelLabel`, `messageId`, `startLine`, `threadId`

## target_detail_get

Fetch one exact chunk of the active editable target plus authoringContext by section. Use this instead of target_get when you only need uiSpec, params, artifact metadata, or compact shapeGraph slices. artifactBundle returns digest fields geometryBackend, edgeTargetCount, faceTargetCount, exportFormats, hasStepExport, and stepExportPath. shapeGraph returns compact parts/instances/constraints/debug/dependencies packets without full source text and includes sourceDigest/coreDigest for guarded follow-up patch flow. Do not promise STEP unless artifactBundle hasStepExport=true or exportArtifacts contains format=step. Use exportArtifacts for STEP path/detail.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `section` (required), `shapeGraphFilters`, `threadId`

## artifact_manifest_get

Fetch the full machine-readable runtime artifact manifest for the active target/model. Returns artifactBundle, modelManifest, digest fields, and runtimeManifestValid after bundle/manifest validation. Use this before export promises or artifact-aware repair.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `threadId`

## artifact_feature_graph_get

Read-only feature/correspondence graph query for the active target/model. Reads the runtime model manifest via model_runtime, so legacy manifests get v0 feature-graph backfill. Returns modelId, artifactDigest, featureGraph, and correspondenceGraph. Does not edit or render.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `threadId`

## target_get

Fetch the full current editable target payload plus artifactDigest. Expensive; prefer target_meta_get, target_macro_get, macro_buffer_get, or target_detail_get unless you truly need everything. Do not promise STEP unless artifactDigest hasStepExport=true or artifactBundle exportArtifacts contains format=step.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`

## ecky_dependency_get

Read-only dependency graph query for sourceLanguage=ecky targets. Supported path shapes: /params/{key} and /targets/{targetId}. Param queries return Core source paths plus impact labels. Target queries return mapped featureIds, parameterKeys, targetIds, and source paths when feature/source bindings exist. Does not edit source or render.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `path` (required), `threadId`

## ecky_selector_resolve

Resolve one selection target id/alias against active target model manifest. Returns durable/canonical ids, bound featureIds/parameterKeys, confidence (exact|inferred|ambiguous|none), plus provenanceCandidates (featureRole, sourceStableNodeKeys, operationKinds, primitiveIds) as best-effort hints. Does not edit source or render.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `targetId` (required), `threadId`

## ecky_constraints_validate

Read-only constraint validation for sourceLanguage=ecky targets. Compiles source and checks CoreParameter min/max/step/choices and params-level :relations (<, <=, >, >=) against provided parameters, or target initial/default parameters. Rows include status/message plus severity, involvedParamKeys, sourceStableNodeKeys, and relation/constraint metadata fields (constraintId, label, kind, sourceStableNodeKey, dependsOnParamKeys, affectsStableNodeKeys). Response also includes authoringLints for repeated anonymous geometry deltas like (+ param N) and (- param N) with suggested parameter names. Does not edit source or render.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `threadId`

## get_model_screenshot

Capture the current model viewport as Ecky can see it. Defaults to the visible workbench view; if the requested target is not open, Ecky asks the user how to proceed.

Arguments: `agentLabel`, `camera`, `includeOverlays`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `timeoutSecs`

## concept_preview_save

Save a concept preview image produced by the connected MCP agent into the current bound thread. Ecky does not call any configured app model or provider for this tool.

Arguments: `agentLabel`, `caption`, `imageData` (required), `llmModelId`, `llmModelLabel`, `messageId`, `threadId`

## params_preview_render

Patch a subset of parameters and rerender a draft. Works without prior browsing by resolving the default target automatically. Returns artifactDigest; check hasStepExport before promising STEP.

Arguments: `agentLabel`, `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameterPatch` (required), `threadId`

## macro_preview_render

Replace macro code and rerender a draft. Returns artifactDigest; check hasStepExport before promising STEP. IMPORTANT: check workspace_overview.agentBrief.summary and rules — if sourceLanguage is `ecky`, macroCode MUST be current `.ecky` source (starting with `(model ...)`). geometryBackend chooses build123d, freecad, or native mesh lowering; source extension does not. Authoring uses pure lispy Ecky source compiled to internal Core IR or the selected backend. `define`, `lambda`, `let`, `let*`, `if`, and generic helpers like `range`, `map`, `filter`, `reduce`, `zip`, `enumerate`, `linspace`, and `flat-map` are allowed; `set!`, assignment, rebinding, and mutation are not. Current `let` bindings are parallel, so same-frame bindings cannot depend on earlier siblings; use `let*` or nested `let` for sequential dependencies. When workspace_overview.agentBrief.summary reports sourceLanguage `ecky`, uiSpec and parameters are auto-derived from the params block. For existing targets, omit parameters: macro_preview_render preserves current target params. Use params_preview_render for numeric changes. parameters only seeds first versions. uiSpec.fields is an array of control descriptors — each field MUST have: key (string), label (string), type (one of: range|number|select|checkbox|image). For numeric parameters, prefer number; range only when explicitly needed. range/number: min, max, step (numbers). select: options array of {label, value} objects — MUST have at least one option. checkbox: no extra fields. image: use for file-picker inputs (e.g. a reference photo) — no extra fields, value is an absolute file path string once chosen by the user. parameters is a flat key→value map matching uiSpec field keys. For image fields, the parameter may be omitted or set to an empty string until the user picks a file in the UI.

Arguments: `agentLabel`, `geometryBackend`, `llmModelId`, `llmModelLabel`, `macroCode` (required), `messageId`, `parameters`, `threadId`, `uiSpec`

## semantic_manifest_get

Fetch a summary of the semantic manifest for the current generated-model target.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`

## semantic_manifest_detail_get

Fetch one exact chunk of the semantic manifest by section.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `section` (required), `threadId`

## control_primitive_save

Create or update one semantic knob and save a new version.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `primitive` (required), `threadId`, `title`, `versionName`

## control_primitive_delete

Delete one semantic knob and save a new version.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `primitiveId` (required), `threadId`, `title`, `versionName`

## control_view_save

Create or update one semantic view and save a new version.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`, `view` (required)

## control_view_delete

Delete one semantic view and save a new version.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`, `viewId` (required)

## measurement_annotation_save

Create or update one measurement semantic annotation and save a new version.

Arguments: `agentLabel`, `annotation` (required), `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`

## measurement_annotation_delete

Delete one measurement semantic annotation and save a new version.

Arguments: `agentLabel`, `annotationId` (required), `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`

## commit_preview_version

Persist the latest green verified preview draft as a new saved version. Call verify_generated_model first; if verification is red, repair and preview again. Do not commit capped red results.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`

## thread_fork_from_target

Save the latest draft or saved target into a new thread.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`

## compare_models

Compare two STL models using build123d comparison engine. Returns volume and bounding box matching metrics.

Arguments: `genPath` (required), `refPath` (required)

## version_restore

Restore an existing saved version.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId` (required)

## user_confirm_request

Show a confirmation dialog with clickable buttons in the Ecky UI. Use this instead of asking in the chat terminal. Blocks until the user responds or the timeout expires.

Arguments: `buttons`, `message` (required), `requestId`, `timeoutSecs`

## request_user_prompt

Request text input from the human in the Ecky UI for a specific thread. Blocks until the user submits or the timeout expires. Prefer thread_borrow/thread_create when choosing a target; pass threadId/messageId explicitly for one-off targeting. Otherwise Ecky uses the current session target from thread_borrow, thread_create, session_log_in, or a prior targeted prompt. Ecky will not guess from the current workspace view. If timeoutSecs is omitted, Ecky uses the configured MCP prompt timeout. The response includes promptText/attachments plus threadId/threadTitle for the target context. Image attachments may include inline dataUrl payloads; prefer those directly and avoid copying them into scratch folders. CAD attachments remain path-based. A timeout is normal when the user does not answer right away; poll again later or call session_log_out if you are leaving the workspace. In active MCP mode, call this again immediately after each completed user-facing turn so Ecky can queue the next message.

Arguments: `message`, `messageId`, `modelId`, `requestId`, `threadId`, `timeoutSecs`

## mark_as_read

Claim queued user thread messages after you inspect them. Pass latestPendingMessageId from thread_list/thread_meta_get, or any pending user message id from thread_get/thread_messages_get; Ecky will drain the whole pending batch for that thread into the current turn.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId` (required), `threadId`

## session_reply_save

Save one final assistant reply into the current thread history. Use this for final user-facing text or fatal turn-ending errors, not for step-by-step progress. After saving the final reply for a turn, immediately call request_user_prompt again.

Arguments: `agentLabel`, `body` (required), `fatal`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`

## session_activity_set

Set the current MCP session activity state so Ecky can drive bubble, microwave, and timer UX without scraping terminal text. Use this for any long or meaningful step.

Arguments: `agentLabel`, `attentionKind`, `detail`, `label`, `llmModelId`, `llmModelLabel`, `phase` (required)

## session_activity_clear

Clear the current explicit MCP session activity state after a step finishes. Optionally set the next phase or idle status text.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `phase`, `statusText`

## long_action_notice

Compatibility alias for session_activity_set. Prefer session_activity_set for new agents.

Arguments: `agentLabel`, `details`, `llmModelId`, `llmModelLabel`, `message` (required), `phase`

## long_action_clear

Compatibility alias for session_activity_clear. Prefer session_activity_clear for new agents.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `phase`, `statusText`

## finalize_thread

Mark a design session as finalized (complete). Moves the thread to inventory. The thread can be re-opened later with reopen.

Arguments: `threadId` (required)

## verify_generated_model

Run deterministic structural verification plus authored `(verify ...)` clauses on the generated model for the currently bound target/thread. Call after preview/render and before commit_preview_version. Returns artifactDigest plus the full structured result including pass/fail, issue codes, metrics, and verifier source. If red, repair source/params and preview again; commit only green verification, or report capped red honestly without commit. Screenshot/VLM verification is secondary.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `originalPrompt`, `threadId`

## get_structural_verification_summary

Lightweight summary of the structural verification result for quick agent routing. Returns artifactDigest, pass/fail, summary text, issue count, and verifier status without full issue details.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `threadId`

## printability_analyze

Read-only printability analysis for the active target/model preview STL. Resolves the current editable target, reads the artifact bundle preview STL path, and returns artifactDigest plus compact mesh/overhang/topology facts. Does not edit source or render.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `threadId`

## printability_transform_recipes_get

Read-only supportless-FDM transform recipe slice for the active target/model preview STL. Returns artifactDigest-guarded candidate recipes with action kind, rationale, estimated effect, target/sourceAnchor when known, and preview/apply support status. Does not edit source or render.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `threadId`

## semantic_transform_preview

Create a source-consistent preview draft for supportless-FDM semantic recipes. Narrow v1 supports actionKind=reorient for sourceLanguage=ecky .ecky sources only, validates expectedArtifact {modelId, previewStlPath, contentHash}, and rejects chamfer/split as unsupported.

Arguments: `actionKind` (required), `agentLabel`, `expectedArtifact` (required), `llmModelId`, `llmModelLabel`, `messageId`, `modelId`, `recipeId` (required), `threadId`

## ecky_ast_get

Experimental AST authoring read for sourceLanguage=ecky. Returns bounded Core AST nodes with stable structural paths, subtree digests, value kinds, spans, authoringContext, and artifactDigest. Optional includeSource returns bounded exact source slices only for source-addressable .ecky nodes. `nodeId` is debug-only and may change across unrelated edits; use `stableNodeKey` as the public handle. Use instead of macro_buffer_get when mcp.eckyAstAuthoring=true.

Arguments: `agentLabel`, `depth`, `includeSource`, `llmModelId`, `llmModelLabel`, `maxNodes`, `messageId`, `path`, `threadId`

## ecky_ast_inspect

Alias for ecky_ast_get. Inspect bounded AST with stable keys and source addressability. `nodeId` is debug-only; use `stableNodeKey` for public references.

Arguments: `agentLabel`, `depth`, `includeSource`, `llmModelId`, `llmModelLabel`, `maxNodes`, `messageId`, `path`, `threadId`

## ecky_ast_get_node

Resolve one exact AST node by stableNodeKey (preferred) or path. Returns a single-node bounded AST payload and optional source slice.

Arguments: `agentLabel`, `includeSource`, `llmModelId`, `llmModelLabel`, `messageId`, `path`, `stableNodeKey`, `threadId`

## ecky_ast_patch_validate

Experimental AST authoring validation for sourceLanguage=ecky. Validates one source-addressable Core AST patch with sourceDigest and expectedNodeDigest guards, resolving stableNodeKey to path when provided, compiles the patched source, and returns compact diff metadata plus best-effort affectedNodeKeys and dependencyImpact summary. Supports replace/insertBefore/insertAfter/delete/rename. Does not render, create a draft, or acquire a lease.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `llmModelId`, `llmModelLabel`, `messageId`, `newName`, `operation`, `path`, `replacementSource`, `sourceDigest` (required), `stableNodeKey`, `threadId`

## ecky_ast_replace_and_render

Experimental AST authoring mutation for sourceLanguage=ecky. Edits one source-addressable Core AST node by stableNodeKey (preferred) or path with sourceDigest and expectedNodeDigest guards, then renders a draft. operation defaults to replace; insertBefore/insertAfter add a sibling around the path; delete removes an arg or keyword pair; rename updates supported binding declarations plus in-scope references. Returns artifactDigest and structuralVerification; check hasStepExport before promising STEP.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `newName`, `operation`, `parameters`, `path`, `postProcessing`, `replacementSource`, `sourceDigest` (required), `stableNodeKey`, `threadId`

## ecky_ast_patch_preview

Alias for ecky_ast_replace_and_render. Apply one guarded AST patch and render preview artifact without committing history.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `newName`, `operation`, `parameters`, `path`, `postProcessing`, `replacementSource`, `sourceDigest` (required), `stableNodeKey`, `threadId`

## ecky_ast_patch_commit

Alias for commit_preview_version. Commit the latest successful preview draft into thread history.

Arguments: `agentLabel`, `llmModelId`, `llmModelLabel`, `messageId`, `threadId`, `title`, `versionName`

## ecky_ast_set_number

Set one numeric literal at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `path` (required), `postProcessing`, `sourceDigest` (required), `threadId`, `value` (required)

## ecky_ast_set_string

Set one string literal at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `path` (required), `postProcessing`, `sourceDigest` (required), `threadId`, `value` (required)

## ecky_ast_set_select

Set one select literal (string/number/boolean) at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `path` (required), `postProcessing`, `sourceDigest` (required), `threadId`, `value` (required)

## ecky_ast_replace_call

Replace one call expression at a source-addressable AST path, then render preview. Wrapper over ecky_ast_replace_and_render operation=replace.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `path` (required), `postProcessing`, `replacementSource` (required), `sourceDigest` (required), `threadId`

## ecky_ast_insert_binding

Insert one binding near the addressed binding path, then render preview. position defaults to after. Wrapper over ecky_ast_replace_and_render operation=insertAfter/insertBefore.

Arguments: `agentLabel`, `bindingSource` (required), `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `path` (required), `position`, `postProcessing`, `sourceDigest` (required), `threadId`

## ecky_ast_delete_binding

Delete one binding at the addressed path, then render preview. Wrapper over ecky_ast_replace_and_render operation=delete.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `parameters`, `path` (required), `postProcessing`, `sourceDigest` (required), `threadId`

## ecky_ast_rename_binding_scoped

Rename one binding and in-scope references, then render preview. Wrapper over ecky_ast_replace_and_render operation=rename.

Arguments: `agentLabel`, `expectedNodeDigest` (required), `geometryBackend`, `llmModelId`, `llmModelLabel`, `messageId`, `newName` (required), `parameters`, `path` (required), `postProcessing`, `sourceDigest` (required), `threadId`
