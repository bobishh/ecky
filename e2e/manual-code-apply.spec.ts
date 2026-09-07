import { expect, test, type Page } from '@playwright/test';

declare global {
  interface Window {
    __manualCodeApplyMock?: {
      addManualVersionCalls: Array<{ input: Record<string, unknown> }>;
      applyManualCodeCalls: Array<{ input: Record<string, unknown> }>;
      applyManualParametersCalls: Array<{ input: Record<string, unknown> }>;
      renderModelCalls: Array<{ macroCode: string; parameters: Record<string, unknown> }>;
      saveProjectSourceCalls: Array<{ threadId: string; source: string }>;
      updateParametersCalls: Array<{ messageId: string; parameters: Record<string, unknown> }>;
      historyCallCount: number;
      draftPreviewCalls: Array<{ threadId: string; previewId: string }>;
      latestThreadId: string | null;
    };
    __manualCodeApplyMockConfig?: {
      stallHistoryAfterCommit?: boolean;
      stallSaveLastDesign?: boolean;
      renderModelError?: string | Record<string, unknown>;
      sourceLanguage?: 'legacyPython' | 'ecky';
      macroCode?: string;
      reuseArtifactIdentity?: boolean;
    };
    __emitTauriEvent?: (event: string, payload: unknown) => void;
  }
}

function manualCodeApplyMockScript() {
  window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
  const eventHandlers = new Map<string, number[]>();
  let nextCallbackId = 1;
  window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
    const callbackId = nextCallbackId++;
    (window as unknown as Record<string, unknown>)[`_${callbackId}`] = callback;
    return callbackId;
  };
  window.__emitTauriEvent = (event, payload) => {
    for (const callbackId of eventHandlers.get(event) ?? []) {
      const callback = (window as unknown as Record<string, unknown>)[`_${callbackId}`];
      if (typeof callback === 'function') {
        callback({ event, id: callbackId, payload });
      }
    }
  };
  window.__manualCodeApplyMock = {
    addManualVersionCalls: [],
    applyManualCodeCalls: [],
    applyManualParametersCalls: [],
    renderModelCalls: [],
    saveProjectSourceCalls: [],
    updateParametersCalls: [],
    historyCallCount: 0,
    draftPreviewCalls: [],
    latestThreadId: null,
  };

  const historyThread = {
    id: 'mock-thread-1',
    title: 'Bracket',
    updatedAt: Date.now() / 1000,
    versionCount: 1,
    pendingCount: 0,
    queuedCount: 0,
    errorCount: 0,
    status: 'ready',
    summary: '',
    messages: [],
  };
  let canonicalSource = window.__manualCodeApplyMockConfig?.macroCode ?? 'print("base bracket")';

  window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'plugin:event|listen') {
      const event = String(args?.event ?? '');
      eventHandlers.set(event, [...(eventHandlers.get(event) ?? []), Number(args?.handler)]);
      return Number(args?.handler);
    }
    if (cmd === 'plugin:event|unlisten') return null;
    if (cmd === 'get_config') {
      return {
        engines: [{ id: 'mock', name: 'Mock' }],
        selectedEngineId: 'mock',
        hasSeenOnboarding: true,
      };
    }
    if (cmd === 'get_runtime_capabilities') {
      return {
        freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
        build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
        mesh: { available: true, detail: 'bundled', path: null },
        recommendedAuthoringContext: {
          engineKind: 'freecad',
          sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad',
        },
      };
    }
    if (cmd === 'check_freecad') return true;
    if (cmd === 'get_history') {
      window.__manualCodeApplyMock!.historyCallCount += 1;
      if (
        window.__manualCodeApplyMockConfig?.stallHistoryAfterCommit &&
        window.__manualCodeApplyMock!.historyCallCount > 1
      ) {
        return new Promise(() => {});
      }
      return [historyThread];
    }
    if (cmd === 'get_last_design') return null;
    if (cmd === 'get_agent_draft_preview') {
      const threadId = String(args?.threadId ?? '');
      const previewId = String(args?.previewId ?? '');
      window.__manualCodeApplyMock!.draftPreviewCalls.push({ threadId, previewId });
      if (previewId === 'mismatched-preview') {
        throw new Error(
          "modelId mismatch: artifactBundle.modelId 'artifact-model-a' conflicts with modelManifest.modelId 'manifest-model-b'.",
        );
      }
      const warningPreview = previewId === 'agent-preview-25';
      const passedPreview = previewId === 'agent-preview-passed';
      const width = warningPreview ? 25 : 33;
      const modelId = warningPreview ? 'agent-preview-model' : 'compact-model';
      return {
        previewId,
        sessionId: 'compact-session',
        threadId,
        baseMessageId: 'mock-msg-1',
        designOutput: {
          title: 'Compact preview', versionName: '', response: '', interactionMode: 'tune',
          macroCode: warningPreview ? 'print("agent draft bracket")' : 'print("compact preview")', sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad', engineKind: 'freecad', uiSpec: { fields: [{ type: 'number', key: 'width', label: 'Width' }] },
          initialParams: { width }, postProcessing: null,
        },
        artifactBundle: {
          modelId, sourceKind: 'generated', sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad', engineKind: 'freecad', contentHash: `${modelId}-hash`,
          artifactVersion: 1, fcstdPath: '', manifestPath: '', macroPath: '',
          modelStlPath: `/mock-${width}.stl`, viewerAssets: [], edgeTargets: [], faceTargets: [],
          calloutAnchors: [], measurementGuides: [], exportArtifacts: [],
        },
        modelManifest: {
          modelId, sourceKind: 'generated', sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad', document: { documentName: 'Compact', documentLabel: 'Compact', objectCount: 0, warnings: [] },
          parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [],
          previewViews: [], advisories: [], selectionTargets: [], measurementAnnotations: [], warnings: [],
          enrichmentState: { status: 'none', proposals: [] },
        },
        draftFeedback: warningPreview || passedPreview ? {
          status: warningPreview ? 'warning' : 'passed',
          summary: warningPreview ? 'Preview requires inspection.' : 'All structural checks passed.',
          items: [],
          source: 'structuralVerification',
        } : null,
        updatedAt: 1, denseTopologyRef: null,
        edgeCount: 0, faceCount: 0, selectionTargetCount: 0, observedBytes: 2048, truncatedFields: [],
      };
    }
    if (cmd === 'create_design_thread') {
      return {
        threadId: 'mock-thread-1',
        sourceDocument: { folder: '/mock/bracket-thread-1', file: '/mock/bracket-thread-1/model.ecky', source: canonicalSource },
        initialVersionId: null, snapshotId: null, parserMatched: null, initialVersionError: null,
        workspace: {
          thread: { id: 'mock-thread-1', title: 'Untitled design', summary: '', updatedAt: 1, versionCount: 0, pendingCount: 0, queuedCount: 0, errorCount: 0, status: 'active', engineKind: 'ecky', sourceLanguage: 'ecky', geometryBackend: 'mesh' },
          messagesPage: { messages: [], nextBefore: null, hasMore: false }, selectedVersion: null, requestedMessageFound: false,
        },
      };
    }
    if (cmd === 'get_project_source') {
      return {
        threadId: String(args?.threadId ?? 'mock-thread-1'),
        slug: 'bracket-thread-1',
        folder: '/mock/bracket-thread-1',
        file: '/mock/bracket-thread-1/model.ecky',
        source: canonicalSource,
      };
    }
    if (cmd === 'save_project_source') {
      const write = {
        threadId: String(args?.threadId ?? ''),
        source: String(args?.source ?? ''),
      };
      window.__manualCodeApplyMock!.saveProjectSourceCalls.push(write);
      canonicalSource = write.source;
      return {
        threadId: write.threadId,
        slug: 'bracket-thread-1',
        folder: '/mock/bracket-thread-1',
        file: '/mock/bracket-thread-1/model.ecky',
        source: canonicalSource,
      };
    }
    if (cmd === 'get_default_macro') return '# mock macro';
    if (cmd === 'init_generation_attempt') {
      window.__manualCodeApplyMock!.latestThreadId = String(args?.threadId ?? '') || null;
      return 'mock-msg-1';
    }
    if (cmd === 'start_exploration_run') {
      const input = args?.input as Record<string, any>;
      window.__manualCodeApplyMock!.latestThreadId = String(input.threadId ?? '') || null;
      const generated = await window.__TAURI_INTERNALS__.invoke('generate_design', {
        prompt: input.prompt,
        threadId: input.threadId,
      });
      const artifactBundle = await window.__TAURI_INTERNALS__.invoke('render_model', {
        macroCode: generated.design.macroCode,
        parameters: generated.design.initialParams,
      });
      const modelManifest = await window.__TAURI_INTERNALS__.invoke('get_model_manifest', {
        modelId: artifactBundle.modelId,
      });
      return {
        run: {
          requestId: input.requestId,
          threadId: generated.threadId,
          phase: 'completed',
          messageId: generated.messageId,
          design: generated.design,
          artifactBundle,
          modelManifest,
          structuralVerification: null,
          usage: null,
          responseText: generated.design.response ?? 'Design synthesized successfully.',
          rawError: null,
          publicationAllowed: true,
        },
        message: {
          id: generated.messageId, role: 'assistant', content: generated.design.response ?? '', status: 'success',
          timestamp: Date.now(), output: generated.design, artifactBundle, modelManifest,
        },
        snapshotId: `snapshot-${generated.messageId}`,
      };
    }
    if (cmd === 'classify_intent') {
      return {
        intentMode: 'design',
        response: 'Routing request...',
        finalResponse: '',
        confidence: 0.9,
        usage: null,
      };
    }
    if (cmd === 'generate_design') {
      const sourceLanguage = window.__manualCodeApplyMockConfig?.sourceLanguage ?? 'legacyPython';
      const macroCode = window.__manualCodeApplyMockConfig?.macroCode ?? 'print("base bracket")';
      const engineKind = sourceLanguage === 'ecky' ? 'ecky' : 'freecad';
      const geometryBackend = sourceLanguage === 'ecky' ? 'build123d' : 'freecad';
      return {
        threadId: 'mock-thread-1',
        messageId: 'mock-msg-1',
        usage: null,
        design: {
          title: 'Bracket',
          versionName: 'V1',
          interactionMode: 'design',
          macroCode,
          sourceLanguage,
          geometryBackend,
          engineKind,
          uiSpec: {
            fields: [
              {
                type: 'number',
                key: 'width',
                label: 'Width',
              },
            ],
          },
          initialParams: {
            width: 10,
          },
          postProcessing: null,
        },
      };
    }
    if (cmd === 'render_model') {
      const sourceLanguage = window.__manualCodeApplyMockConfig?.sourceLanguage ?? 'legacyPython';
      const engineKind = sourceLanguage === 'ecky' ? 'ecky' : 'freecad';
      const geometryBackend = sourceLanguage === 'ecky' ? 'build123d' : 'freecad';
      const payload = {
        macroCode: String(args?.macroCode ?? ''),
        parameters: (args?.parameters as Record<string, unknown>) ?? {},
      };
      window.__manualCodeApplyMock?.renderModelCalls.push(payload);
      if (window.__manualCodeApplyMockConfig?.renderModelError) {
        const error = window.__manualCodeApplyMockConfig.renderModelError;
        throw typeof error === 'string' ? new Error(error) : error;
      }
      const renderIndex = window.__manualCodeApplyMockConfig?.reuseArtifactIdentity
        ? 1
        : window.__manualCodeApplyMock?.renderModelCalls.length ?? 1;
      return {
        modelId: `mock-model-${renderIndex}`,
        sourceKind: 'generated',
        sourceLanguage,
        geometryBackend,
        engineKind,
        contentHash: `mock-hash-${renderIndex}`,
        fcstdPath: `/mock-${renderIndex}.FCStd`,
        manifestPath: `/mock-${renderIndex}/manifest.json`,
        modelStlPath: `/mock-${renderIndex}.stl`,
        viewerAssets: [],
        calloutAnchors: [],
        measurementGuides: [],
        edgeTargets: [],
      };
    }
    if (cmd === 'apply_manual_code') {
      const input = (args?.input as Record<string, any>) ?? {};
      window.__manualCodeApplyMock!.applyManualCodeCalls.push({ input });
      const persist = Boolean(input.persist);
      const sourceLanguage = input.sourceLanguage ?? window.__manualCodeApplyMockConfig?.sourceLanguage ?? 'legacyPython';
      const geometryBackend = input.geometryBackend ?? (sourceLanguage === 'ecky' ? 'build123d' : 'freecad');
      const engineKind = sourceLanguage === 'ecky' ? 'ecky' : 'freecad';
      const messageId = persist
        ? `manual-intent-msg-${window.__manualCodeApplyMock!.applyManualCodeCalls.length}`
        : null;
      if (window.__manualCodeApplyMockConfig?.renderModelError) {
        const configured = window.__manualCodeApplyMockConfig.renderModelError;
        const message = typeof configured === 'string' ? configured : String(configured.message ?? configured);
        return {
          threadId: input.threadId,
          baseMessageId: input.baseMessageId ?? null,
          messageId,
          status: 'error',
          designOutput: {
            title: input.title, versionName: input.versionName, response: 'Manual code draft failed.',
            interactionMode: 'design', macroCode: input.source, sourceLanguage, geometryBackend, engineKind,
            uiSpec: input.uiSpec, initialParams: input.parameters, postProcessing: input.postProcessing ?? null,
          },
          artifactBundle: null,
          modelManifest: null,
          parserMatched: false,
          error: { code: 'render', message },
        };
      }
      const renderIndex = window.__manualCodeApplyMockConfig?.reuseArtifactIdentity
        ? 1
        : window.__manualCodeApplyMock!.applyManualCodeCalls.length;
      const modelId = `mock-model-${renderIndex}`;
      const artifactBundle = {
        modelId, sourceKind: 'generated', sourceLanguage, geometryBackend, engineKind,
        contentHash: `mock-hash-${renderIndex}`, artifactVersion: 1,
        fcstdPath: `/mock-${renderIndex}.FCStd`, manifestPath: `/mock-${renderIndex}/manifest.json`,
        modelStlPath: `/mock-${renderIndex}.stl`, viewerAssets: [], exportArtifacts: [],
        calloutAnchors: [], measurementGuides: [], edgeTargets: [], faceTargets: [],
      };
      const modelManifest = {
        schemaVersion: 1, modelId, sourceKind: 'generated', sourceLanguage, geometryBackend, engineKind,
        document: { documentName: 'Bracket', documentLabel: 'Bracket', objectCount: 0, warnings: [] },
        parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [],
        previewViews: [], selectionTargets: [], advisories: [], measurementAnnotations: [], warnings: [],
        enrichmentState: { status: 'none', proposals: [] },
      };
      const write = { threadId: String(input.threadId), source: String(input.source) };
      window.__manualCodeApplyMock!.saveProjectSourceCalls.push(write);
      canonicalSource = write.source;
      return {
        threadId: input.threadId,
        baseMessageId: input.baseMessageId ?? null,
        messageId,
        status: 'success',
        designOutput: {
          title: input.title, versionName: input.versionName,
          response: persist ? 'Manual edit appended as new version.' : 'Code draft applied.',
          interactionMode: 'design', macroCode: input.source, sourceLanguage, geometryBackend, engineKind,
          uiSpec: input.uiSpec, initialParams: input.parameters, postProcessing: input.postProcessing ?? null,
        },
        artifactBundle,
        modelManifest,
        snapshotId: `snapshot-${modelId}`,
        parserMatched: true,
        error: null,
      };
    }
    if (cmd === 'apply_manual_parameters') {
      const input = (args?.input as Record<string, any>) ?? {};
      window.__manualCodeApplyMock!.applyManualParametersCalls.push({ input });
      const sourceLanguage = window.__manualCodeApplyMockConfig?.sourceLanguage ?? 'legacyPython';
      const geometryBackend = sourceLanguage === 'ecky' ? 'build123d' : 'freecad';
      const engineKind = sourceLanguage === 'ecky' ? 'ecky' : 'freecad';
      const renderIndex = window.__manualCodeApplyMock!.applyManualParametersCalls.length + 10;
      const modelId = `mock-model-${renderIndex}`;
      return {
        threadId: input.threadId,
        baseMessageId: input.targetMessageId,
        messageId: input.persist ? `manual-param-msg-${renderIndex}` : null,
        status: 'success',
        designOutput: {
          title: 'Bracket', versionName: 'V1', response: 'Parameters applied.', interactionMode: 'design',
          macroCode: canonicalSource, sourceLanguage, geometryBackend, engineKind,
          uiSpec: { fields: [{ type: 'number', key: 'width', label: 'Width' }] },
          initialParams: input.parameters, postProcessing: null,
        },
        artifactBundle: {
          modelId, sourceKind: 'generated', sourceLanguage, geometryBackend, engineKind,
          contentHash: `mock-hash-${renderIndex}`, artifactVersion: 1, fcstdPath: '',
          manifestPath: `/mock-${renderIndex}/manifest.json`, modelStlPath: `/mock-${renderIndex}.stl`,
          viewerAssets: [], exportArtifacts: [], edgeTargets: [], faceTargets: [], calloutAnchors: [], measurementGuides: [],
        },
        modelManifest: {
          schemaVersion: 1, modelId, sourceKind: 'generated', sourceLanguage, geometryBackend, engineKind,
          document: { documentName: 'Bracket', documentLabel: 'Bracket', objectCount: 0, warnings: [] },
          parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [], previewViews: [],
          selectionTargets: [], advisories: [], measurementAnnotations: [], warnings: [],
          enrichmentState: { status: 'none', proposals: [] },
        },
        snapshotId: `snapshot-${modelId}`,
        error: null,
      };
    }
    if (cmd === 'get_model_manifest') {
      const sourceLanguage = window.__manualCodeApplyMockConfig?.sourceLanguage ?? 'legacyPython';
      const geometryBackend = sourceLanguage === 'ecky' ? 'build123d' : 'freecad';
      return {
        modelId: String(args?.modelId ?? 'mock-model-1'),
        sourceKind: 'generated',
        sourceLanguage,
        geometryBackend,
        document: {
          documentName: 'Bracket',
          documentLabel: 'Bracket',
          objectCount: 0,
          warnings: [],
        },
        parts: [],
        parameterGroups: [],
        controlPrimitives: [],
        controlRelations: [],
        controlViews: [],
        selectionTargets: [],
        advisories: [],
        measurementAnnotations: [],
        warnings: [],
        enrichmentState: { status: 'none', proposals: [] },
      };
    }
    if (cmd === 'verify_generated_model') {
      return {
        passed: true,
        summary: 'Checks passed.',
        issues: [],
        metrics: {
          partCount: 1,
          modelStlSizeBytes: 1024,
          totalVolume: 1000,
          totalArea: 500,
          bbox: { xMin: 0, yMin: 0, zMin: 0, xMax: 10, yMax: 10, zMax: 10 },
        },
        verifierStatus: 'ok',
        verifierSource: 'mock',
      };
    }
    if (cmd === 'verify_render') {
      return {
        passed: true,
        summary: 'Visual checks passed.',
        issues: [],
        usage: null,
      };
    }
    if (cmd === 'get_thread') {
      return {
        ...historyThread,
        id: String(args?.id ?? historyThread.id),
      };
    }
    if (cmd === 'add_manual_version') {
      window.__manualCodeApplyMock?.addManualVersionCalls.push({
        input: (args?.input as Record<string, unknown>) ?? {},
      });
      return `manual-msg-${window.__manualCodeApplyMock?.addManualVersionCalls.length ?? 1}`;
    }
    if (cmd === 'update_parameters') {
      window.__manualCodeApplyMock?.updateParametersCalls.push({
        messageId: String(args?.messageId ?? ''),
        parameters: (args?.parameters as Record<string, unknown>) ?? {},
      });
      return null;
    }
    if (
      cmd === 'update_post_processing' ||
      cmd === 'update_version_runtime' ||
      cmd === 'save_model_manifest' ||
      cmd === 'finalize_generation_attempt' ||
      cmd === 'save_config'
    ) {
      return null;
    }
    if (cmd === 'save_last_design') {
      if (window.__manualCodeApplyMockConfig?.stallSaveLastDesign) {
        return new Promise(() => {});
      }
      return null;
    }
    if (cmd === 'get_active_agent_sessions') return [];
    if (cmd === 'get_agent_terminal_snapshots') return [];
    if (cmd === 'get_agent_activity') return { events: [], latestCursor: 0 };
    if (cmd === 'get_thread_agent_state') {
      return {
        threadId: args?.threadId ?? null,
        connectionState: 'disconnected',
        sessions: [],
        primaryAgentLabel: null,
        statusText: '',
      };
    }
    return null;
  };
}

async function bootManualCodeFlow(page: Page) {
  let stlRequestCount = 0;
  await page.route(/\/mock-\d+\.stl(?:\?.*)?$/, async (route) => {
    stlRequestCount += 1;
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: `solid mock
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid mock
`,
    });
  });
  await page.addInitScript(manualCodeApplyMockScript);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await page.fill('textarea.prompt-input', 'make bracket');
  await page.locator('textarea.prompt-input').press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');
  await expect
    .poll(() => page.evaluate(() => window.__manualCodeApplyMock?.renderModelCalls.length ?? 0))
    .toBeGreaterThan(0);
  await page.getByRole('button', { name: 'Parameters', exact: true }).click({ force: true });
  const paramPanel = page.locator('.param-panel');
  await expect(paramPanel).toBeVisible({ timeout: 10000 });
  await page.locator('[data-window-id="params"] .window-header').click({ force: true });
  const rawButton = paramPanel.getByRole('button', { name: 'RAW' });
  if (await rawButton.count()) await rawButton.click({ force: true });
  await expect(paramPanel.locator('[data-param-key="width"]')).toBeVisible();
  return () => stlRequestCount;
}

test.describe('Manual code apply/version coverage', () => {
  test('Given a passed MCP preview When it becomes active Then Ecky presents success instead of warning', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    await page.evaluate(() => {
      const activeThreadId = window.__manualCodeApplyMock?.latestThreadId;
      if (!activeThreadId) throw new Error('Expected active generation thread');
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'agent-session',
        threadId: activeThreadId,
        previewId: 'agent-preview-passed',
        baseMessageId: 'mock-msg-1',
        modelId: 'compact-model',
        revision: 26,
        feedbackStatus: 'passed',
        feedbackSummary: 'All structural checks passed.',
      });
    });

    const card = page.locator('.agent-notification-center .agent-card').filter({ hasText: 'All structural checks passed.' });
    await expect(card).toBeVisible();
    await expect(card.locator('.agent-card__severity')).toHaveText('success');
    await expect(card).toHaveClass(/agent-card--success/);
    await expect(card).not.toHaveClass(/agent-card--error/);
  });

  test('Given warning MCP preview becomes active When Code opens Then UI shows the rendered params and matching source', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    await page.evaluate(() => {
      const activeThreadId = window.__manualCodeApplyMock?.latestThreadId;
      if (!activeThreadId) throw new Error('Expected active generation thread');
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'agent-session',
        threadId: activeThreadId,
        previewId: 'agent-preview-25',
        baseMessageId: 'mock-msg-1',
        modelId: 'agent-preview-model',
        revision: 25,
        feedbackStatus: 'warning',
        feedbackSummary: 'Preview requires inspection.',
      });
    });

    const warningCard = page.locator('.agent-notification-center .agent-card').filter({ hasText: 'Preview requires inspection.' });
    await expect(warningCard).toBeVisible();
    await expect(warningCard.locator('.agent-card__severity')).toHaveText('warning');
    await expect(warningCard).toHaveClass(/agent-card--warning/);

    const widthInput = page.locator('[data-param-key="width"] input[type="number"]').first();
    await expect(widthInput).toHaveValue('25');

    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal.locator('.cm-content')).toContainText('print("agent draft bracket")');
    await expect(modal.locator('.cm-content')).not.toContainText('print("base bracket")');
    await expect(modal.getByTestId('code-draft-source-notice')).toContainText(
      'RENDERED VIEWPORT SOURCE',
    );
    await expect(modal.getByRole('button', { name: 'OPEN BASE FILE' })).toBeVisible();
  });

  test('Given compact preview invalidations When active, background, and stale events arrive Then only newest active preview hydrates', async ({ page }) => {
    await bootManualCodeFlow(page);
    await page.evaluate(() => {
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'compact-session', threadId: 'background-thread', previewId: 'background-preview',
        baseMessageId: null, modelId: 'background-model', revision: 1,
      });
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'compact-session', threadId: 'mock-thread-1', previewId: 'active-preview',
        baseMessageId: 'mock-msg-1', modelId: 'compact-model', revision: 2,
      });
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'compact-session', threadId: 'mock-thread-1', previewId: 'stale-preview',
        baseMessageId: 'mock-msg-1', modelId: 'stale-model', revision: 1,
      });
    });

    await expect(page.locator('[data-param-key="width"] input[type="number"]').first()).toHaveValue('33');
    const calls = await page.evaluate(() => window.__manualCodeApplyMock?.draftPreviewCalls ?? []);
    expect(calls).toEqual([{ threadId: 'mock-thread-1', previewId: 'active-preview' }]);
  });

  test('Given Code is open When bound Ecky source starts rendering Then editor shows that exact source', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        sourceLanguage: 'ecky',
        macroCode: '(model (part old (box 1 1 1)))',
      };
    });
    await bootManualCodeFlow(page);

    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal.locator('.cm-content')).toContainText('(part old');

    const renderedSource = '(model (part stamp (box 60 45 3)))';
    await page.evaluate(async (source) => {
      await window.__TAURI_INTERNALS__.invoke('save_project_source', {
        threadId: 'mock-thread-1',
        source,
      });
      window.__emitTauriEvent?.('project-folder-sync', [{
        kind: 'detected',
        threadId: 'mock-thread-1',
        slug: 'bracket-thread-1',
      }]);
    }, renderedSource);

    await expect(modal.locator('.cm-content')).toContainText(renderedSource);
    await expect(modal.locator('.cm-content')).not.toContainText('(part old');
  });

  test('Given another thread publishes a preview When current thread stays active Then its workspace remains unchanged', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    await page.evaluate(() => {
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'background-agent-session',
        threadId: 'background-thread',
        previewId: 'background-preview-99',
        baseMessageId: null,
        modelId: 'background-preview-model',
        revision: 99,
        feedbackStatus: 'warning',
        feedbackSummary: 'Background preview finished.',
      });
    });

    const widthInput = page.locator('[data-param-key="width"] input[type="number"]').first();
    await expect(widthInput).toHaveValue('10');
    await expect(page.locator('.agent-notification-center')).not.toContainText(
      'Background preview finished.',
    );
    const calls = await page.evaluate(() => window.__manualCodeApplyMock?.draftPreviewCalls ?? []);
    expect(calls).not.toContainEqual({ threadId: 'background-thread', previewId: 'background-preview-99' });
  });

  test('Given an active preview mixes artifact and manifest identities When it arrives Then the last good workspace remains visible', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    await page.evaluate(() => {
      const activeThreadId = window.__manualCodeApplyMock?.latestThreadId;
      if (!activeThreadId) throw new Error('Expected active generation thread');
      window.__emitTauriEvent?.('agent-draft-preview-changed', {
        sessionId: 'agent-session',
        threadId: activeThreadId,
        previewId: 'mismatched-preview',
        baseMessageId: 'mock-msg-1',
        modelId: 'artifact-model-a',
        revision: 77,
      });
    });

    const widthInput = page.locator('[data-param-key="width"] input[type="number"]').first();
    await expect(widthInput).toHaveValue('10');
    await expect(page.getByRole('alert')).toContainText('artifact-model-a');
    await expect(page.getByRole('alert')).toContainText('manifest-model-b');

    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal.locator('.cm-content')).toContainText('print("base bracket")');
    await expect(modal.locator('.cm-content')).not.toContainText('print("mismatch")');
  });

  test('Given edited version source When Apply runs Then one Rust intent owns render and immutable commit', async ({ page }) => {
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal).toBeVisible();
    await expect(modal.getByRole('button', { name: 'INSERT VERIFY' })).toHaveCount(0);
    const editor = page.locator('.cm-content').first();
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("draft bracket")');

    await modal.getByRole('button', { name: 'APPLY', exact: true }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => ({
          addManualVersionCount: window.__manualCodeApplyMock?.addManualVersionCalls.length ?? -1,
          savedSource: window.__manualCodeApplyMock?.saveProjectSourceCalls.at(-1) ?? null,
          manualCode: window.__manualCodeApplyMock?.applyManualCodeCalls.at(-1) ?? null,
        })),
      )
      .toMatchObject({
        addManualVersionCount: 0,
        savedSource: {
          threadId: 'mock-thread-1',
          source: 'print("draft bracket")',
        },
        manualCode: {
          input: {
            source: 'print("draft bracket")',
            parameters: { width: 10 },
            persist: true,
          },
        },
      });

    await expect(modal).toBeHidden();
    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i }).click();
    await expect(page.locator('.cm-content').first()).toContainText('print("draft bracket")');
  });

  test('Given Apply reuses backend artifact identity When draft renders Then viewport reloads the STL', async ({ page }) => {
    await bootManualCodeFlow(page);
    const viewerShell = page.locator('.viewer-shell');
    await expect(viewerShell).toHaveAttribute('data-model-key', /.+/);
    const modelKeyBeforeApply = await viewerShell.getAttribute('data-model-key');
    await page.evaluate(() => {
      window.__manualCodeApplyMockConfig = { reuseArtifactIdentity: true };
    });

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    const editor = modal.locator('.cm-content');
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("same identity, new draft")');
    await modal.getByRole('button', { name: 'APPLY', exact: true }).click();

    await expect.poll(() => viewerShell.getAttribute('data-model-key')).not.toBe(modelKeyBeforeApply);
    await expect(modal.locator('.commit-error')).toHaveCount(0);
  });

  test('Given code render fails before source publication When inspector reopens Then last rendered source remains visible', async ({ page }) => {
    await bootManualCodeFlow(page);
    await page.evaluate(() => {
      window.__manualCodeApplyMockConfig = { renderModelError: 'mock apply render exploded' };
    });

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    const editor = modal.locator('.cm-content');
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("failed but canonical")');
    await modal.getByRole('button', { name: 'APPLY', exact: true }).click();

    await expect(modal.locator('.commit-error')).toContainText('mock apply render exploded');
    await expect.poll(() => page.evaluate(() => window.__manualCodeApplyMock?.saveProjectSourceCalls.length ?? -1))
      .toBe(0);

    await modal.locator('.window-close').click();
    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i }).click();
    await expect(page.locator('.cm-content').first()).toContainText('print("base bracket")');
    await expect(page.locator('.cm-content').first()).not.toContainText('print("failed but canonical")');
  });

  test('Given applied code draft When macro patch event exists Then code editor shows LAST MACRO DIFF with actor and changed lines', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal).toBeVisible();
    await expect(modal.getByTestId('last-macro-diff')).toHaveCount(0);

    const editor = page.locator('.cm-content').first();
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("diffed bracket")');
    await modal.getByRole('button', { name: 'APPLY', exact: true }).click();
    await expect(modal).toBeHidden();
    await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i }).click();
    await expect(modal).toBeVisible();

    const diffPanel = modal.getByTestId('last-macro-diff');
    await expect(diffPanel).toBeVisible();
    await expect(diffPanel.getByTestId('last-macro-diff-meta')).toContainText('SYSTEM');
    await expect(diffPanel.getByTestId('last-macro-diff-meta')).toContainText('line');
    await expect(diffPanel.getByTestId('last-macro-diff-summary')).toContainText('Manual');
    await expect(diffPanel.getByTestId('last-macro-diff-rows')).toContainText('print("diffed bracket")');
  });

  test('Given ecky workbench code When verify template inserts and applies Then one Rust intent commits authored verify source', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        sourceLanguage: 'ecky',
        macroCode: '(model)',
      };
    });
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal).toBeVisible();
    await expect(modal.getByRole('button', { name: 'INSERT VERIFY' })).toBeVisible();
    await modal.getByRole('button', { name: 'INSERT VERIFY' }).click();
    await expect(modal.getByRole('button', { name: 'VERIFY INSERTED' })).toBeDisabled();
    await expect(modal.locator('.cm-content')).toContainText('(verify');

    await modal.getByRole('button', { name: 'APPLY', exact: true }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => ({
          addManualVersionCount: window.__manualCodeApplyMock?.addManualVersionCalls.length ?? -1,
          manualCode: window.__manualCodeApplyMock?.applyManualCodeCalls.at(-1) ?? null,
        })),
      )
      .toMatchObject({
        addManualVersionCount: 0,
        manualCode: {
          input: {
            source:
            '(model\n' +
            '  (verify\n' +
            '    (tag body_shell)\n' +
            '    (metric check (manifest has-step))\n' +
            '    (expect check (= true)))\n' +
            ')\n',
            persist: true,
            parameters: { width: 10 },
          },
        },
      });
  });

  test('Given ecky workbench code with two parts When verify template inserts Then clearance template is used', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        sourceLanguage: 'ecky',
        macroCode: '(model\n  (part body (box 1 1 1))\n  (part lid (box 1 1 1)))',
      };
    });
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal).toBeVisible();
    await modal.getByRole('button', { name: 'INSERT VERIFY' }).click();
    await expect(modal.locator('.cm-content')).toContainText('clearance min-distance body lid');
    await expect(modal.locator('.cm-content')).toContainText('body_lid_gap');
  });

  test('Given ecky workbench code When code modal opens Then ecky syntax tokens are highlighted', async ({ page }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        sourceLanguage: 'ecky',
        macroCode:
          '; shell\n' +
          '(model\n' +
          '  (params\n' +
          '    (number width 10 :label "Width")))\n',
      };
    });
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal).toBeVisible();
    await expect(modal.locator('.cm-ecky-comment').filter({ hasText: '; shell' })).toBeVisible();
    await expect(modal.locator('.cm-ecky-keyword').filter({ hasText: 'model' })).toBeVisible();
    await expect(modal.locator('.cm-ecky-kind').filter({ hasText: 'number' })).toBeVisible();
    await expect(modal.locator('.cm-ecky-number').filter({ hasText: '10' })).toBeVisible();
    await expect(modal.locator('.cm-ecky-string').filter({ hasText: '"Width"' })).toBeVisible();
    await expect(modal.locator('.cm-ecky-atom').filter({ hasText: ':label' })).toBeVisible();
  });

  test('Given params changed and code edited When commit runs Then one Rust intent owns immutable version creation', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    const widthInput = page.locator('[data-param-key="width"] input[type="number"]').first();
    await widthInput.fill('42');
    await page.locator('.param-panel').getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => (
          window.__manualCodeApplyMock?.applyManualParametersCalls.at(-1)?.input as {
            parameters?: { width?: number };
          } | undefined
        )?.parameters?.width ?? null),
      )
      .toBe(42);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const editor = page.locator('.cm-content').first();
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("edited bracket")');

    await page.getByLabel('Version title').fill('Final Bracket');
    await page.getByLabel('Version name').fill('V-fit');
    await page
      .locator('[role="dialog"]')
      .filter({ hasText: 'MACRO INSPECTOR:' })
      .getByRole('button', { name: 'APPLY', exact: true })
      .click();

    await expect
      .poll(async () =>
        page.evaluate(() => ({
          addManualVersionCount: window.__manualCodeApplyMock?.addManualVersionCalls.length ?? -1,
          manualCode: window.__manualCodeApplyMock?.applyManualCodeCalls.at(-1) ?? null,
        })),
      )
      .toMatchObject({
        addManualVersionCount: 0,
        manualCode: {
          input: {
            title: 'Final Bracket',
            versionName: 'V-fit',
            source: 'print("edited bracket")',
            parameters: { width: 42 },
            persist: true,
          },
        },
      });
  });

  test('Given ecky workbench code When verify template inserts and commits Then committed version keeps authored verify source', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        sourceLanguage: 'ecky',
        macroCode: '(model)',
      };
    });
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const modal = page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' });
    await expect(modal).toBeVisible();
    await modal.getByRole('button', { name: 'INSERT VERIFY' }).click();
    await modal.getByLabel('Version title').fill('Verified Bracket');
    await modal.getByLabel('Version name').fill('V-verify');
    await modal.getByRole('button', { name: 'APPLY', exact: true }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => ({
          addManualVersionCount: window.__manualCodeApplyMock?.addManualVersionCalls.length ?? -1,
          manualCode: window.__manualCodeApplyMock?.applyManualCodeCalls.at(-1) ?? null,
        })),
      )
      .toMatchObject({
        addManualVersionCount: 0,
        manualCode: {
          input: {
            title: 'Verified Bracket',
            versionName: 'V-verify',
            source:
              '(model\n' +
              '  (verify\n' +
              '    (tag body_shell)\n' +
              '    (metric check (manifest has-step))\n' +
              '    (expect check (= true)))\n' +
              ')\n',
            parameters: { width: 10 },
            persist: true,
          },
        },
      });
  });

  test('Given post-commit refresh stalls When committing Then UI exits COMMITTING state after core save', async ({ page }) => {
    await bootManualCodeFlow(page);
    await page.evaluate(() => {
      window.__manualCodeApplyMockConfig = {
        stallHistoryAfterCommit: true,
        stallSaveLastDesign: true,
      };
    });

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const editor = page.locator('.cm-content').first();
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("edited bracket")');

    await page
      .locator('[role="dialog"]')
      .filter({ hasText: 'MACRO INSPECTOR:' })
      .getByRole('button', { name: 'APPLY', exact: true })
      .click();

    await expect
      .poll(async () => page.evaluate(() => window.__manualCodeApplyMock?.applyManualCodeCalls.filter(call => call.input.persist).length ?? 0))
      .toBe(1);
    // Windows stay mounted when closed (visibility:hidden), so assert
    // hidden-ness rather than absence from the DOM.
    await expect(
      page.locator('[role="dialog"]').filter({ hasText: 'MACRO INSPECTOR:' }),
    ).toBeHidden();
    await expect(
      page.locator('.commit-actions').getByRole('button', { name: 'COMMITTING...' }),
    ).toHaveCount(0);
  });

  test('Given first render fails When closing and reopening from viewport code button Then failed draft stays editable without a successful model', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        renderModelError: 'mock render exploded',
      };
    });
    await page.route(/\/mock-\d+\.stl(?:\?.*)?$/, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body: `solid mock
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid mock
`,
      });
    });
    await page.addInitScript(manualCodeApplyMockScript);
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    await page.fill('textarea.prompt-input', 'make broken bracket');
    await page.locator('textarea.prompt-input').press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    await expect(page.getByText(/MACRO INSPECTOR:/i)).toBeVisible();
    await expect(page.locator('.cm-content').first()).toContainText('print("base bracket")');
    await expect(page.locator('.error-banner')).toHaveCount(0);
    await expect(page.locator('.agent-notification-center')).toContainText('mock render exploded');

    await page
      .locator('[role="dialog"]')
      .filter({ hasText: 'MACRO INSPECTOR:' })
      .locator('.window-close')
      .click();

    const viewportCodeButton = page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i });
    await expect(viewportCodeButton).toBeVisible();
    await expect(viewportCodeButton).toBeEnabled();

    await viewportCodeButton.click();

    await expect(page.getByText(/MACRO INSPECTOR:/i)).toBeVisible();
    await expect(page.locator('.cm-content').first()).toContainText('print("base bracket")');
  });

  test('Given a Core IR authoring error When generation render fails Then Ecky keeps raw text with layer and fix', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      window.__manualCodeApplyMockConfig = {
        sourceLanguage: 'ecky',
        renderModelError: {
          code: 'validation',
          message: 'Unknown operation `spher`.',
          layer: 'coreIr',
          fix: {
            hint: 'Use a supported Core IR operation.',
            suggestions: ['sphere'],
          },
        },
      };
    });
    await page.addInitScript(manualCodeApplyMockScript);
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    await page.fill('textarea.prompt-input', 'make a broken sphere');
    await page.locator('textarea.prompt-input').press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    const notification = page.locator('.agent-notification-center .agent-card').filter({ hasText: 'Unknown operation `spher`.' });
    await expect(notification).toContainText('CORE IR');
    await expect(notification).toContainText('Use a supported Core IR operation. Try: sphere');
  });
});
