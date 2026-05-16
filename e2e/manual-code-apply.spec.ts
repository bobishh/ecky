import { expect, test, type Page } from '@playwright/test';

declare global {
  interface Window {
    __manualCodeApplyMock?: {
      addManualVersionCalls: Array<{ input: Record<string, unknown> }>;
      renderModelCalls: Array<{ macroCode: string; parameters: Record<string, unknown> }>;
      updateParametersCalls: Array<{ messageId: string; parameters: Record<string, unknown> }>;
    };
  }
}

function manualCodeApplyMockScript() {
  window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
  window.__manualCodeApplyMock = {
    addManualVersionCalls: [],
    renderModelCalls: [],
    updateParametersCalls: [],
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

  window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
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
    if (cmd === 'get_history') return [historyThread];
    if (cmd === 'get_last_design') return null;
    if (cmd === 'get_default_macro') return '# mock macro';
    if (cmd === 'init_generation_attempt') return 'mock-msg-1';
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
      return {
        threadId: 'mock-thread-1',
        messageId: 'mock-msg-1',
        usage: null,
        design: {
          title: 'Bracket',
          versionName: 'V1',
          interactionMode: 'design',
          macroCode: 'print("base bracket")',
          sourceLanguage: 'legacyPython',
          geometryBackend: 'freecad',
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
      const payload = {
        macroCode: String(args?.macroCode ?? ''),
        parameters: (args?.parameters as Record<string, unknown>) ?? {},
      };
      window.__manualCodeApplyMock?.renderModelCalls.push(payload);
      const renderIndex = window.__manualCodeApplyMock?.renderModelCalls.length ?? 1;
      return {
        modelId: `mock-model-${renderIndex}`,
        sourceKind: 'generated',
        sourceLanguage: 'legacyPython',
        geometryBackend: 'freecad',
        engineKind: 'freecad',
        contentHash: `mock-hash-${renderIndex}`,
        fcstdPath: `/mock-${renderIndex}.FCStd`,
        manifestPath: `/mock-${renderIndex}/manifest.json`,
        previewStlPath: `/mock-${renderIndex}.stl`,
        viewerAssets: [],
        calloutAnchors: [],
        measurementGuides: [],
        edgeTargets: [],
      };
    }
    if (cmd === 'get_model_manifest') {
      return {
        modelId: String(args?.modelId ?? 'mock-model-1'),
        sourceKind: 'generated',
        sourceLanguage: 'legacyPython',
        geometryBackend: 'freecad',
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
          previewStlSizeBytes: 1024,
          totalVolume: 1000,
          totalArea: 500,
          bbox: { xMin: 0, yMin: 0, zMin: 0, xMax: 10, yMax: 10, zMax: 10 },
        },
        verifierStatus: 'ok',
        verifierSource: 'mock',
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
      cmd === 'save_last_design' ||
      cmd === 'save_config'
    ) {
      return null;
    }
    if (cmd === 'get_active_agent_sessions') return [];
    if (cmd === 'get_agent_terminal_snapshots') return [];
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
  await page.fill('textarea.prompt-input', 'make bracket');
  await page.locator('textarea.prompt-input').press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');
  await page.getByRole('button', { name: 'PARAMS' }).click({ force: true });
  await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
  await page.getByRole('button', { name: 'RAW' }).click();
  await expect(page.locator('[data-param-key="width"]')).toBeVisible();
}

test.describe('Manual code apply/version coverage', () => {
  test('Given edited code draft When applying without commit Then render uses current params and add_manual_version stays untouched', async ({ page }) => {
    await bootManualCodeFlow(page);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    await expect(page.getByText(/MACRO INSPECTOR:/i)).toBeVisible();
    const editor = page.locator('.cm-content').first();
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("draft bracket")');

    await page.locator('.code-modal-footer').getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => ({
          addManualVersionCount: window.__manualCodeApplyMock?.addManualVersionCalls.length ?? -1,
          renderModel: window.__manualCodeApplyMock?.renderModelCalls.at(-1) ?? null,
        })),
      )
      .toMatchObject({
        addManualVersionCount: 0,
        renderModel: {
          macroCode: 'print("draft bracket")',
          parameters: { width: 10 },
        },
      });
  });

  test('Given params changed and code edited When commit creates new version Then add_manual_version uses latest params and chosen title/version', async ({
    page,
  }) => {
    await bootManualCodeFlow(page);

    const widthInput = page.locator('[data-param-key="width"] input[type="number"]').first();
    await widthInput.fill('42');
    await page.getByRole('button', { name: 'APPLY' }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => window.__manualCodeApplyMock?.renderModelCalls.at(-1)?.parameters.width ?? null),
      )
      .toBe(42);

    await page.locator('.param-panel').getByRole('button', { name: 'CODE' }).click();
    const editor = page.locator('.cm-content').first();
    await editor.click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
    await page.keyboard.type('print("edited bracket")');

    await page.getByLabel('Version title').fill('Final Bracket');
    await page.getByLabel('Version name').fill('V-fit');
    await page.locator('.code-modal-footer').getByRole('button', { name: 'COMMIT VERSION' }).click();

    await expect
      .poll(async () =>
        page.evaluate(() => ({
          addManualVersion: window.__manualCodeApplyMock?.addManualVersionCalls.at(-1) ?? null,
          renderModel: window.__manualCodeApplyMock?.renderModelCalls.at(-1) ?? null,
        })),
      )
      .toMatchObject({
        addManualVersion: {
          input: {
            title: 'Final Bracket',
            versionName: 'V-fit',
            macroCode: 'print("edited bracket")',
            parameters: { width: 42 },
          },
        },
        renderModel: {
          macroCode: 'print("edited bracket")',
          parameters: { width: 42 },
        },
      });
  });
});
