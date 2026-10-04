# Local Trace Audit: 29 September 2026

## Source and Method

Read 51 `run.edn`, `trajectory.edn`, and `report.md` sets under the local application's
`evals/runs` directory. Count tool events once per retained provider step identity,
merging active/done state transitions. Compare adjacent operation names separately
from retained input digests. Inspect original Agy command records for the concrete
Python example; do not execute any recorded command.

This is a read-only audit. No provider configuration, project, model, or identity
was changed. Local raw paths and full conversation payloads are not copied into
this repository report.

## Observations

All 51 retained runs identify provider `agy`. Status distribution: 16 success,
22 error, 13 interrupted. These statuses are recorded transport/run outcomes, not
an independent quality assessment of the produced designs.

There are 1,380 unique retained tool invocations:

| Operation | Count |
| --- | ---: |
| run_command | 442 |
| view_file | 268 |
| ecky_mcp/params_preview_render | 132 |
| manage_task | 130 |
| replace_file_content | 92 |
| call_mcp_tool | 50 |

The 442 commands are 32.0% of retained invocations. Commands are not uniformly
wasteful: retained examples include numerical calculations, model writes, reading
source files, downloading evidence, and diagnostic inspection.

All 51 reports mark tokens and USD cost as unknown. These artifacts cannot establish
measured savings or whether particular command sequences exhausted provider quota.

Adjacent calls with the same name and retained input digest: manage_task 102,
view_file 19, params_preview_render 3, run_command 2, replace_file_content 2.
Matching digest does not prove matching full arguments or unchanged project state:
retained arguments may already be shortened, and result data is sometimes absent.
These counts identify review candidates, not an execution denylist.

The newest retained run has five web searches with different arguments and no
new version. Its report's four adjacent repetitions mean repeated tool names,
not five identical requests. Blanket duplicate-name rejection would misclassify it.

## Confirmed Python Pseudo-Checks

Original transcript records from 24 September contain four consecutive command
requests proposing successive catch/arch shapes. Each Python program contains
comments plus exactly one print of a literal success statement. AST inspection
confirms no calculation, geometry construction, constraint evaluation, or assertion.
Following result records show successful process exit and the printed statement.

The agent's tool summaries describe these as geometry tests. Process exit zero
proves the print ran; it does not verify geometry. The next command writes actual
model source and checks parenthesis counts, which is a different operation and
must not be collapsed into these four no-op checks.

This supports a concrete regression fixture: print-only pseudo-verification is
not verification evidence. It does not support removing Python in general.

## Codex Coverage Gap At Audit Baseline

`commands/agy_provider.rs` constructs completed/failed EvalRun records and calls
`llm_eval::persist_run`. The inspected Codex queue and app-server adapter contain
no corresponding run construction/persistence path. The app-server references
`llm_eval::format_tool_call_details` for presentation, which is not trace capture.

Codex can therefore have successful persisted dialogue and artifact versions while
these eval files contain no Codex runs. Missing evals do not mean failed work or
missing conversation history. Add terminal, interrupted, and failed Codex trajectory
capture to the existing file format, including invocation identity and version refs.

## Remaining Questions and Decisions

- Retained payload truncation and absent results limit whole-sequence judgments.
  Use original records or mark evidence incomplete before labeling an action wasteful.
- Follow-up inspection found 20 terminal error runs with nonempty answers; 11
  contain `RESOURCE_EXHAUSTED (code 429): Individual quota reached` from the Gemini
  Cloud Code route. Other retained failures include 502/503 and TCP resets. These
  records support quota/transport failures, not a claim of unexpected API-key
  switching. The Agy finalizer previously persisted answers only on SUCCESS.
  Preserve answer and actual error independently; do not claim provider success.
- Keep hard action rules in Rust. Jev selects intent/model; no per-call classifier
  approval in the initial change. Optional action assessment starts shadow-only.
- Replay useful computation and changed-input validation alongside the confirmed
  pseudo-check. Prevent false blocks, not only reduce tool counts.
- Do not infer cost savings from these reports until classifier and provider usage
  and comparable task completion are measured.

## Implementation Review Boundary

The new Codex adapter feeds the existing strict-EDN/Markdown writer from queued
turns. Integration proof covers terminal statuses, app-server exit, and production
queue dispatch/version linkage. Capture bounds and incomplete events remain explicit.
This does not add Jev settings or routing. Active/unflushed capture is memory-only:
full Ecky process crash recovery for that evidence remains unfinished.

Review proof on 2026-09-29: Codex projection suite 17/17; production queue/version
linkage and persistence-retry test 1/1; capture-limit test 1/1; Agy adapter 13/13.
Reported model/effort is retained with either response/notification ordering.
Malformed start responses produce error evidence and cannot leak their seed into
the next turn. Formatting, diff checks, and strict validation of both OpenSpec
changes passed. These are targeted local checks, not a claim of full CI or deployment.

## Installed Codex Native Hook Protocol Probe

An isolated Codex 0.153.4 CLI run on 2026-09-29 used a loopback Responses fixture
to force `exec_command`, `view_image`, and `request_user_input`. One vetted
temporary hook denied all three. Returned tool outputs explicitly reported
PreToolUse rejection; the shell sentinel was absent. Native hook names were
`Bash`, `view_image`, and `request_user_input` respectively. Every hook payload
included `session_id`, `turn_id`, and `tool_use_id`; the first two identifiers
matched across calls in the same turn. This supports exact active-turn correlation,
rather than weakening the guard when a field has not yet been observed.

Evidence: `/tmp/ecky-native-payload-probe-20260929/hooks.jsonl` and
`requests.jsonl`. Only metadata and synthetic tool outputs were retained; this
probe used no account credentials or live model API. Invocation-scoped hook-trust
bypass applied only to the isolated home containing that reviewed probe hook.
It does not establish trust for Ecky's production hook, callback integration,
other tools, or hosted tool paths. Those remain separate acceptance requirements.

A second isolated native TUI probe used no trust bypass. The startup review screen
opened the sole hook's detail view: PreToolUse, matcher `.*`, session-flags source,
the temporary probe command, synchronous execution, five-second timeout. Trust
was granted to that individual definition through the native review control.
After closing the CLI, a fresh app-server with the same cwd and startup override
returned `trustStatus: trusted` and the unchanged definition hash through
`hooks/list`. Evidence:
`/tmp/ecky-hook-review-probe-20260929/trusted-hooks-list.json`.
No user Codex home, account, trust database, or unrelated hook was modified.
Ecky can direct users to review its exact owned command through this supported
flow; this does not add a new approval UI or establish production coverage.

A subsequent isolated CLI run reused that persisted trust without a bypass.
A forced `apply_patch` call reached PreToolUse and was denied before creating its
patch sentinel. Evidence: `/tmp/ecky-native-extra-probe-20260929/requests.jsonl`
and the appended hook metadata in the native-payload probe. An invalid
`write_stdin` process identifier failed before the hook; that case provides no
coverage proof for interaction with a valid running subprocess.

A valid-process follow-up did establish a gap. The fixture allowed only the
initial `Bash` invocation to start `/bin/cat` with a PTY. The next forced
`write_stdin` call fed `PROBE_FEED` into that exact returned process identifier;
both terminal echo and cat output were observed. The hook log contained only
the initial Bash invocation, although the hook's default decision for every
subsequent call was deny. Evidence:
`/tmp/ecky-stdin-hook-probe-20260929/requests.jsonl` and `hooks.jsonl`.
The earlier system-Python shim run is retained separately and is not the clean
fixture proof. This means the hook alone cannot implement a no-tools contract.
The routed unified-exec restriction must be verified against a valid process,
and disabling the experiment must restore the user's actual prior configuration.

An isolated installed-CLI feature-boundary probe then set both
`features.unified_exec=false` and `features.shell_tool=false`. `write_stdin` was
absent from the actual offered tool catalog; forcing the call returned
`unsupported call: write_stdin`, rather than entering the process-ID lookup or
input delivery path. Evidence:
`/tmp/ecky-disabled-stdin-probe-20260929/requests.jsonl`. This establishes the CLI
feature boundary; Ecky's thread-resume scope and restoration still need their
own integration proof. It does not turn the hook into a complete interceptor.
