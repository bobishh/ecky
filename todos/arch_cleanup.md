# Architectural Cleanup - COMPLETE

Goal: move Drydemacher from patch-driven orchestration to explicit invariant-driven flow.

## Principles

- [x] One authoritative app flow state, not overlapping booleans
- [x] One backend-owned context builder
- [x] One request pipeline per user action
- [x] Explicit invariants for question vs design vs render retry
- [x] UI components mostly presentational
- [x] Errors modeled as state transitions where possible, not ad hoc control flow

## Phases

- [x] Phase 1: Make State Explicit (Single Phase Model)
- [x] Phase 2: Extract Session Flow Controller (`sessionFlow.ts`)
- [x] Phase 3: Separate Domain State From View State (`domainState.ts`, `viewState.ts`)
- [x] Phase 4: Backend Owns Prompt Context
- [x] Phase 5: Formalize Invariants (Added to SessionFlow)
- [x] Phase 6: Extract Boot/Restore Flow into dedicated coordinator (`boot.ts`)
- [x] Phase 7: Extract Microwave Audio/Effects (`microwave.ts`)
- [x] Phase 8: Improve Observability (Structured logging)
- [x] Phase 9: Pay Down Existing Reactive Debt (`ParamPanel.svelte` warning fixed)

## Definition Of Done

- [x] `src/App.svelte` is materially smaller and mostly declarative
- [x] Request lifecycle is readable in one place
- [x] State transitions are explicit
- [x] Context building is bounded and centralized
- [x] No known duplicate-run path remains
- [x] New feature work is cheaper than it is today
