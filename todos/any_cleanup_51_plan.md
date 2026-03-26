# Any Cleanup Plan (51 places)

Target: remove or strictly constrain all current `any` usages in `src/`.

## 1) `src/lib/ParamPanel.svelte` (1)
- [ ] `src/lib/ParamPanel.svelte:68` Replace comment wording (`any`) to avoid false-positive noise, keep intent unchanged.

## 2) `src/lib/audio/microwave.ts` (10)
- [ ] `src/lib/audio/microwave.ts:33` Replace `(window as any).webkitAudioContext` with a typed window extension.
- [ ] `src/lib/audio/microwave.ts:116` Introduce `AppConfig` type and replace `config: any`.
- [ ] `src/lib/audio/microwave.ts:130` Type asset callback parameter instead of `(a: any)`.
- [ ] `src/lib/audio/microwave.ts:143` Replace `nodes: any[]` with explicit node union type.
- [ ] `src/lib/audio/microwave.ts:251` Replace `config: any` in `startMicrowaveAudio`.
- [ ] `src/lib/audio/microwave.ts:262` Replace `config: any` in `playDing`.
- [ ] `src/lib/audio/microwave.ts:267` Type asset lookup callback instead of `(a: any)`.
- [ ] `src/lib/audio/microwave.ts:296` Replace `config: any` in `playErrorBuzz`.
- [ ] `src/lib/audio/microwave.ts:321` Replace `config: any` in `startRequestHum`.
- [ ] `src/lib/audio/microwave.ts:325` Replace `config: any` in `stopRequestHum`.

## 3) `src/lib/boot/restore.ts` (6)
- [ ] `src/lib/boot/restore.ts:44` Replace `invoke<any>('get_config')` with `invoke<AppConfig>`.
- [ ] `src/lib/boot/restore.ts:49` Replace `(e: any)` in engine predicate with typed engine model.
- [ ] `src/lib/boot/restore.ts:90` Replace `(e: any)` in selected engine lookup with typed engine model.
- [ ] `src/lib/boot/restore.ts:122` Replace `invoke<any[]>('get_history')` with `invoke<Thread[]>`.
- [ ] `src/lib/boot/restore.ts:134` Replace `invoke<[any, string] | null>('get_last_design')` with typed tuple.
- [ ] `src/lib/boot/restore.ts:141` Replace `invoke<any>('get_thread')` with `invoke<Thread>`.

## 4) `src/lib/controllers/manualController.ts` (1)
- [ ] `src/lib/controllers/manualController.ts:14` Replace `newParams: any` with `Record<string, unknown>` or `DesignParams`.

## 5) `src/lib/controllers/requestOrchestrator.ts` (11)
- [ ] `src/lib/controllers/requestOrchestrator.ts:57` Replace `viewerRef: any` with explicit viewer interface.
- [ ] `src/lib/controllers/requestOrchestrator.ts:58` Replace `openCodeModalManual: any` with function signature type.
- [ ] `src/lib/controllers/requestOrchestrator.ts:63` Replace `viewerComponent: any` in deps with viewer interface.
- [ ] `src/lib/controllers/requestOrchestrator.ts:64` Replace callback payload `data: any` with typed modal payload.
- [ ] `src/lib/controllers/requestOrchestrator.ts:107` Replace `attachments: any[]` with `Attachment[]`.
- [ ] `src/lib/controllers/requestOrchestrator.ts:159` Replace `currentConfig: any` with `AppConfig`.
- [ ] `src/lib/controllers/requestOrchestrator.ts:282` Replace `invoke<any>('generate_design')` with `invoke<GenerateOutput>`.
- [ ] `src/lib/controllers/requestOrchestrator.ts:358` Replace `invoke<any>('classify_intent')` with `invoke<IntentDecision>`.
- [ ] `src/lib/controllers/requestOrchestrator.ts:410` Replace `data: any` with typed fallback payload.
- [ ] `src/lib/controllers/requestOrchestrator.ts:454` Replace `design?: any` with `DesignOutput | undefined`.
- [ ] `src/lib/controllers/requestOrchestrator.ts:469` Replace `err: any` with `unknown` + normalized error helper.

## 6) `src/lib/stores/history.ts` (2)
- [ ] `src/lib/stores/history.ts:167` Replace `invoke<any[]>('get_history')` with `invoke<Thread[]>`.
- [ ] `src/lib/stores/history.ts:172` Replace `invoke<any>('get_thread')` with `invoke<Thread>`.

## 7) `src/lib/stores/paramPanelState.ts` (9)
- [ ] `src/lib/stores/paramPanelState.ts:7` Replace `normalizeUiSpec(uiSpec: any)` with `UiSpec` input type.
- [ ] `src/lib/stores/paramPanelState.ts:13` Replace `normalizeParams(params: any)` with `DesignParams` input type.
- [ ] `src/lib/stores/paramPanelState.ts:22` Replace `Record<string, any>` with `Record<string, unknown>` or numeric/bool union.
- [ ] `src/lib/stores/paramPanelState.ts:38` Replace `uiSpec?: any` with `uiSpec?: UiSpec`.
- [ ] `src/lib/stores/paramPanelState.ts:39` Replace `params?: Record<string, any>` with typed params map.
- [ ] `src/lib/stores/paramPanelState.ts:49` Replace `hydrateFromVersion(design: any, ...)` with `DesignOutput`.
- [ ] `src/lib/stores/paramPanelState.ts:66` Replace `setUiSpec(uiSpec: any)` with `setUiSpec(uiSpec: UiSpec)`.
- [ ] `src/lib/stores/paramPanelState.ts:70` Replace `setParams(params: Record<string, any>)` with typed params map.
- [ ] `src/lib/stores/paramPanelState.ts:74` Replace `patchParams(partialParams: Record<string, any>)` with typed params map.

## 8) `src/lib/stores/requestQueue.ts` (3)
- [ ] `src/lib/stores/requestQueue.ts:19` Replace `attachments: any[]` with `Attachment[]`.
- [ ] `src/lib/stores/requestQueue.ts:29` Replace `design: any` with `DesignOutput | null`.
- [ ] `src/lib/stores/requestQueue.ts:74` Replace `attachments: any[]` in `submit` signature with `Attachment[]`.

## 9) `src/lib/stores/sessionStore.ts` (5)
- [ ] `src/lib/stores/sessionStore.ts:42` Replace `(fn: any)` in `phase` facade with typed subscriber callback.
- [ ] `src/lib/stores/sessionStore.ts:43` Replace `(fn: any)` in `status` facade with typed subscriber callback.
- [ ] `src/lib/stores/sessionStore.ts:44` Replace `(fn: any)` in `error` facade with typed subscriber callback.
- [ ] `src/lib/stores/sessionStore.ts:45` Replace `(fn: any)` in `stlUrl` facade with typed subscriber callback.
- [ ] `src/lib/stores/sessionStore.ts:46` Replace `(fn: any)` in `isManual` facade with typed subscriber callback.

## 10) `src/lib/types/domain.ts` (3)
- [ ] `src/lib/types/domain.ts:8` Replace `fields: any[]` with `UiField[]`.
- [ ] `src/lib/types/domain.ts:10` Replace `initialParams: Record<string, any>` with typed params map.
- [ ] `src/lib/types/domain.ts:32` Replace `genieTraits?: any` with explicit `GenieTraits` shape.

## Delivery order
- [ ] Phase A: `domain.ts` + shared type aliases (`UiField`, `UiSpec`, `DesignParams`, `AppConfig`, `IntentDecision`, `GenerateOutput`).
- [ ] Phase B: Replace typed `invoke<T>` in `boot/restore`, `history`, and orchestrator commands.
- [ ] Phase C: Refactor stores (`requestQueue`, `sessionStore`, `paramPanelState`) to zero `any`.
- [ ] Phase D: Refactor controllers (`manualController`, `requestOrchestrator`) to zero `any`.
- [ ] Phase E: Refactor audio module and remove remaining `any` in callbacks/nodes/window extension.
- [ ] Phase F: Add CI gate (`npm run typecheck`) and fail on new `any` regressions.
