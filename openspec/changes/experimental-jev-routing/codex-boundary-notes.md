# Historical Codex boundary evidence

Superseded routing design. Evidence below describes the previous Codex-only
implementation and probes; it does not define the current global routing contract.
Hook activation is not a prerequisite for Jev. Current requirements are in
`proposal.md`, `design.md`, `specs/jev-routing/spec.md`, and `tasks.md`.

## Tool Boundaries and Optional Human Confirmation

Keep deterministic policy authoritative. MCP discovery is a convenience; validate
again at execution for cached/direct calls. Bind permission to exact active turn,
project, artifact revision, operation, arguments, and permission subset.

Codex pre-tool hooks can block supported local calls, while app-server approval
requests handle actions requiring permission. Neither covers every possible tool:
hosted tools and some paths bypass hooks; approval events do not occur for every
in-sandbox command. Test installed protocol support and actual coverage. Hooks may
deny, but their currently unsupported ask decision is not a confirmation UI.

The installed CLI probe proves a vetted temporary PreToolUse hook can deny a
shell call before execution. It does not establish production trust or full tool
coverage. Codex merges hook sources and requires review of exact non-managed hook
definitions. Ecky must not bypass that review for unrelated user hooks, rewrite
the global trust database, install system-wide managed policy, or migrate the
user's Codex account to another home directory. A required missing/untrusted
boundary fails the queued routed attempt before `turn/start`. Provider feature
flags are accepted as a boundary only after actual tool-catalog and forced-call
probes establish their effect; the current CLI probe still exposed `view_image`
after `tools.view_image = false`.

An installed app-server probe found that thread-level hook overrides do not appear
in `hooks/list`. A startup `-c hooks.PreToolUse=...` override in the Ecky-owned
Codex child does appear as `sessionFlags`, with its exact definition hash, but is
initially untrusted. Registration therefore belongs to the owned process startup;
thread overrides are not evidence of interception. Trust verification must match
the bound project directory, command, matcher, synchronous mode, timeout, source,
and current hash. Registration and native execution coverage are separate proofs.
The hook command must remain stable across endpoint restarts; an owned private
descriptor can resolve the existing dynamic callback endpoint. Hook setup must
not add a fixed-port dependency to experiment-disabled MCP startup. Review of the
exact owned definition uses Codex's supported review flow. An isolated native TUI
probe reviewed and trusted one startup hook individually; after closing the CLI,
a new app-server with the same startup override and cwd returned its unchanged
hash as trusted. This proves the supported review workflow, not that a user has
already reviewed Ecky's production executable definition.

### Activating the experimental boundary

Keep Ecky running. Enable the classifier and save the token through Settings.
If the first queued attempt reports an untrusted startup hook, use the concrete
review command in that diagnostic. It selects the same Codex executable, bound
project directory, and exact startup hook override as Ecky's owned process; its
arguments are shell-quoted and contain no TypeSafe token or callback capability.
In that CLI session, open `/hooks`, inspect the exact Ecky PreToolUse definition,
and trust only that definition. Close the review session and retry the failed queue
item in Ecky. Do not use a hook-trust bypass or trust unrelated definitions.

A plain Codex invocation without the supplied startup override does not load this
session-flags hook. If the executable path or hook definition changes, the exact
definition needs review again. The queued turn remains failed until the current
definition is enabled and trusted. This is provider-native review, not an Ecky
approval UI or an automatic authorization decision.

A separate isolated installed-CLI protocol probe denied forced shell,
`view_image`, and `request_user_input` calls through PreToolUse and observed
`session_id`, `turn_id`, and `tool_use_id` in their actual payloads. The callback
must retain exact active-turn identity checks. This probe used an invocation-scoped
trust bypass for its sole vetted temporary hook; it is not production trust proof
and does not establish interception of hosted tools or other untested paths.

A forced `write_stdin` call to a valid `/bin/cat` PTY bypassed PreToolUse in the
installed CLI. Keep routed native feature restrictions until their actual handler
effect is proven; interception of process creation does not cover later input.
Scoped restrictions must restore the user's effective pre-route configuration on
the next experiment-disabled turn. Do not guess default booleans or treat an
omitted field in a resume request as evidence that a sticky override was removed.

Installed app-server probes established that null overrides do not remove sticky
feature restrictions. `experimentalFeature/list` with the loaded thread ID
reports effective feature enablement; restoring those concrete booleans restored
shell execution in the same durable thread. Use canonical `features.view_image`,
not `tools.view_image`. Read every page and fail before dispatch when a required
feature state is unavailable. The official configuration documentation specifies
cached web search by default, with live search for full-access sandboxes; preserve
an explicit mode and resolve an omitted mode against the actual sandbox rather
than assuming `config/read` null means disabled or cached.
Source: https://learn.chatgpt.com/docs/config-file/config-basic#web-search-mode.

Further installed CLI probes, using the individually reviewed hook without a
trust bypass, intercepted `get_goal`, `create_goal`, and `update_goal` before
execution. These establish coverage for those three native handlers, not all
possible native or hosted tools. The freshly built Ecky executable and actual Rust
HTTP callback subsequently passed an end-to-end fixture covering unbound denial,
active Answer denial, stale-turn denial, Inspect answer-first gating and permitted
read, plus ordinary prompt-based passthrough. The fixture caught and corrected an
Axum 0.8 route-registration panic (`:token` versus `{token}`) before acceptance.
It does not replace the installed CLI interception probes or a live Jev API run.

If human confirmation is added, route supported app-server requests to a scoped
pending state with exact action preview and Allow/Deny. Handle cancellation,
disconnect, expiry, and stale turn replies. No response grants nothing. A denied
policy cannot be bypassed by repeating or reframing the tool call. Changes to
prompt, arguments, target, or revision invalidate the grant. Do not allow a child
process to become a parallel authority or use blanket session grants.

Routine allowed work should not acquire a confirmation on every call. First fix
deterministic policy and trace evidence. Jev action review, if later evaluated,
starts in shadow mode and cannot authorize actions, expand a sandbox, or override
user denial. Its API cost and latency must earn their place.

