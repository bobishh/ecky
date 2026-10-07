# Change: Add Antigravity provider adapter

## Why

Ecky Provider mode currently supports only Codex and stores only one provider
binding per Ecky thread. Users need Antigravity (`agy`) as another owned,
streaming provider conversation without losing Codex ownership or canonical Ecky
context when switching modes.

## What Changes

- Add `AGY` beside `CODEX` in Provider settings as `provider:agy`.
- Generalize binding and FIFO ownership to one binding per Ecky thread and provider.
- Add a long-lived Antigravity stream-json adapter with version gating, live progress,
  durable local transcript pages including user attachments, resume, raw errors, and stop.
- Inject Ecky thread identity, canonical handoff, project cwd, and workspace MCP
  configuration into the first Antigravity turn.
- Expose adapter capabilities so Dialogue does not offer unsupported steering.
- Omit unsupported voice input; stream-json documents text blocks only, and Agy
  dialogue must not route recordings to standalone NVIDIA Speech.

## Impact

- Affected specs: `codex-agent-takeover`, new `agy-provider-adapter`.
- Affected code: Settings, Dialogue routing, provider persistence, Tauri commands,
  Antigravity process supervisor.
- Requires Antigravity CLI `>=1.1.15` for bidirectional stream-json.
