# Voice Interaction Plan

**Date:** 2026-04-30

## Goal

Voice should make Ecky feel like a geometry tutor, not a chat reader.

Primary demo loop:

```text
user speaks goal
  -> Ecky transcribes intent
  -> context digest feeds model/agent
  -> Ecky answers with concise mentor copy
  -> TTS speaks that copy
  -> viewport highlights geometry
  -> renderer builds validated CAD
```

## V1 Scope

- Use push-to-talk or explicit text input; no always-listening mode.
- Transcribe push-to-talk through a separate speech adapter, not the LLM adapter.
- NVIDIA Speech path uses hosted Riva gRPC with the configured NVIDIA NIM API key.
- NVIDIA Speech runtime is bundled as `runtime/speech`; users must not install Python packages manually.
- STT language code lives in Settings -> Sounds and defaults to `en-US`.
- Speak only concise Ecky bubble copy.
- Never speak raw MCP transcripts, terminal logs, or full backend errors.
- Mute control is always visible in the workbench and gates all app audio.
- Persist mute state through existing config save path.
- Use native WebView speech synthesis first; OS voices are enough for demo.

## Context Fed To AI

Use digest, not transcript:

- user goal
- selected thread/version
- visible bubble state
- selected geometry and params
- current `.ecky` or backend source excerpt
- validation ledger summary
- raw latest backend/provider error when relevant
- viewport/projection evidence

## Later Scope

- Native Rust gRPC client for NVIDIA Speech, replacing the bundled Python Riva bridge.
- Per-voice settings: voice, rate, pitch, volume.
- Geometry-grounded narration timeline: point, axis, plane, sketch, extrude, cut.
- 3Blue1Brown-style Math Lens overlays tied to source ranges.
- Local/offline TTS option after demo path proves useful.

## Non-Goals

- No ambient microphone by default.
- No transcript narration.
- No separate agent status bar.
- No voice path that bypasses validators or source patches.
