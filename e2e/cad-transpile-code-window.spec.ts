import { expect, test, type Page } from '@playwright/test';

type Mode = 'success' | 'pending' | 'error';

const FOREIGN_SOURCE = 'cube([10, 20, 30]);\ncylinder(h = 6, r = 3, $fn = 6);';
const ECKY_SOURCE = '(model\n  (part body (box 10 20 30))\n  (verify (check stl connected-component-count = 1)))';

async function installMocks(page: Page, mode: Mode, provider = false) {
  await page.addInitScript(({ mode, eckySource, provider }) => {
    const mockWindow = window as any;
    localStorage.clear();
    mockWindow.__CAD_TRANSPILE_CALLS__ = [];
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.metadata = {};
    window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
      const id = Math.floor(Math.random() * 1_000_000_000);
      mockWindow[`_${id}`] = callback;
      return id;
    };
    window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
      mockWindow.__CAD_TRANSPILE_CALLS__.push({ cmd, args });
      if (cmd === 'plugin:event|listen') return Number(args?.handler ?? 1);
      if (cmd === 'plugin:event|unlisten') return null;
      if (cmd === 'get_config') {
        return {
          engines: [{ id: 'mock', name: 'Mock', provider: 'openai', apiKey: 'key', model: 'mock-model', baseUrl: 'http://mock', enabled: true }],
          selectedEngineId: 'mock',
          freecadCmd: '',
          assets: [],
          microwave: { humId: null, dingId: null, muted: true },
          voice: { sttLanguageCode: 'en-US' },
          mcp: { mode: 'passive', primaryAgentId: null, promptTimeoutSecs: 1800, autoAgents: [] },
          hasSeenOnboarding: true,
          connectionType: provider ? 'provider:agy' : null,
          providerModels: { codex: '', agy: 'claude-sonnet-4-6', grok: '' },
          defaultEngineKind: 'build123d',
          defaultSourceLanguage: 'ecky',
          defaultGeometryBackend: 'build123d',
          maxGenerationAttempts: 1,
          maxVerifyAttempts: 0,
        };
      }
      if (cmd === 'save_config') return null;
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          mesh: { available: true, detail: 'Ready', path: null },
          recommendedAuthoringContext: { engineKind: 'build123d', sourceLanguage: 'ecky', geometryBackend: 'build123d' },
        };
      }
      if (cmd === 'get_history') return [];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_default_macro') return '';
      if (cmd === 'get_agent_activity') return { events: [], latestCursor: 0 };
      if (cmd === 'get_active_agent_sessions' || cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'project_folder_render_activity') return [];
      if (cmd === 'create_design_thread') {
        return {
          threadId: 'transpile-thread',
          sourceDocument: { folder: '/mock/projects/transpile-thread', file: '/mock/projects/transpile-thread/model.ecky', source: '(model (part body (box 10 10 10)))' },
          initialVersionId: null, snapshotId: null, parserMatched: null, initialVersionError: null,
          workspace: {
            thread: { id: 'transpile-thread', title: 'Untitled design', summary: '', updatedAt: 1, versionCount: 0, pendingCount: 0, queuedCount: 0, errorCount: 0, status: 'active', engineKind: 'ecky', sourceLanguage: 'ecky', geometryBackend: 'mesh' },
            messagesPage: { messages: [], nextBefore: null, hasMore: false }, selectedVersion: null, requestedMessageFound: false,
          },
        };
      }
      if (cmd === 'get_project_source') {
        return {
          threadId: 'transpile-thread',
          slug: 'transpile-thread',
          folder: '/mock/projects/transpile-thread',
          file: '/mock/projects/transpile-thread/model.ecky',
          source: '(model (part body (box 10 10 10)))',
        };
      }
      if (cmd === 'get_thread_agent_state') return { threadId: args?.threadId ?? null, connectionState: 'disconnected', sessions: [], primaryAgentLabel: null, statusText: '' };
      if (cmd === 'get_agy_provider') return null;
      if (cmd === 'get_agy_provider_messages') return { messages: [], nextCursor: null, backwardsCursor: null };
      if (cmd === 'send_agy_provider_prompt') {
        if (mode === 'error') {
          throw { code: 'provider', message: 'AGY request rejected', details: 'RAW_AGY_BODY: conversation unavailable' };
        }
        return {
          binding: {
            eckyThreadId: String((args?.input as any)?.eckyThreadId ?? 'transpile-thread'),
            agyConversationId: 'agy-transpile',
          },
          capabilities: { steer: false, stop: true },
          messages: [],
          liveMessages: [],
          turnTraces: [],
          nextCursor: null,
          backwardsCursor: null,
          runtime: { phase: 'running', activeTurnId: 'agy-turn-1', error: null },
          queue: [],
        };
      }
      if (cmd === 'init_generation_attempt') return 'transpile-message';
      if (cmd === 'classify_intent') return { intentMode: 'design', confidence: 1, response: 'Translating CAD source.', finalResponse: null, usage: null };
      if (cmd === 'generate_design') {
        if (mode === 'pending') await new Promise((resolve) => setTimeout(resolve, 700));
        if (mode === 'error') {
          throw { code: 'provider', message: 'NIM request rejected', details: 'RAW_NIM_BODY: model unavailable in region eu-1' };
        }
        return {
          threadId: args?.threadId ?? 'transpile-thread',
          messageId: 'transpile-message',
          usage: null,
          design: {
            title: 'Transpiled model',
            versionName: 'V1',
            response: 'CAD source translated.',
            interactionMode: 'design',
            macroCode: eckySource,
            macroDialect: 'ecky',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
        };
      }
      if (cmd === 'render_model') {
        return {
          modelId: 'transpiled-model', sourceKind: 'generated', sourceLanguage: 'ecky', geometryBackend: 'build123d', engineKind: 'ecky',
          contentHash: 'hash', artifactVersion: 1, fcstdPath: '', manifestPath: '/mock/manifest.json', macroPath: '/mock/model.ecky', modelStlPath: '/mock/transpiled.stl',
          viewerAssets: [], calloutAnchors: [], measurementGuides: [], edgeTargets: [], faceTargets: [],
        };
      }
      if (cmd === 'get_model_manifest') {
        return {
          modelId: 'transpiled-model', sourceKind: 'generated', sourceLanguage: 'ecky', geometryBackend: 'build123d', engineKind: 'ecky',
          document: { documentName: 'Transpiled model', documentLabel: 'Transpiled model', objectCount: 1, warnings: [] },
          parts: [{ partId: 'body', label: 'body', kind: 'solid', editable: true, parameterKeys: [] }],
          parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [], selectionTargets: [], advisories: [], measurementAnnotations: [], warnings: [], enrichmentState: { status: 'none', proposals: [] },
        };
      }
      if (cmd === 'verify_generated_model') {
        return {
          passed: true, summary: 'Structural and authored verification passed.', issues: [],
          authoredVerifyChecks: [{ tag: 'connected', status: 'passed', message: 'component count = 1' }],
          metrics: { partCount: 1, modelStlComponentCount: 1, modelStlNonManifoldEdgeCount: 0 },
          verifierStatus: 'okRustOnly', verifierSource: 'rust_structural',
        };
      }
      if (cmd === 'verify_render') return { passed: true, issues: '', usage: null };
      if (cmd === 'save_model_manifest' || cmd === 'finalize_generation_attempt' || cmd === 'save_last_design' || cmd === 'update_version_preview') return null;
      if (cmd === 'get_thread') return { id: args?.id ?? 'transpile-thread', title: 'Transpiled model', updatedAt: Date.now() / 1000, versionCount: 1, pendingCount: 0, errorCount: 0, summary: '', messages: [] };
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      return null;
    };
  }, { mode, eckySource: ECKY_SOURCE, provider });
  await page.route(/\/mock\/(?:transpiled|mess)\.stl(?:\?.*)?$/, (route) => route.fulfill({
    status: 200,
    contentType: 'model/stl',
    body: 'solid mock\nendsolid mock\n',
  }));
}

async function openForeignCode(page: Page) {
  await page.goto('/');
  await page.getByTestId('workbench-bottom-dock').getByRole('button', { name: 'CODE' }).click();
  const modal = page.locator('[data-window-id="code"]');
  await expect(modal).toBeVisible();
  const editor = modal.locator('.cm-content');
  await editor.click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
  await page.keyboard.insertText(FOREIGN_SOURCE);
  await expect(editor).toContainText('cube([10, 20, 30])');
  return { modal, editor };
}

test('Given API config When foreign CAD translates Then normal configured send owns dispatch', async ({ page }) => {
  await installMocks(page, 'success');
  const { modal } = await openForeignCode(page);

  await modal.getByRole('button', { name: 'TRANSLATE TO ECKY' }).click();
  await expect(modal).toBeHidden();
  await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();

  const generateCall = await page.evaluate(() => (window as any).__CAD_TRANSPILE_CALLS__.find((call: any) => call.cmd === 'generate_design'));
  expect(generateCall.args.prompt).toContain(FOREIGN_SOURCE);
  expect(generateCall.args.prompt).toContain('Translate the foreign CAD source');
});

test('Given AGY provider When foreign FreeCAD translates Then normal provider dialogue owns dispatch', async ({ page }) => {
  await installMocks(page, 'success', true);
  const { modal } = await openForeignCode(page);

  await modal.getByRole('button', { name: 'TRANSLATE TO ECKY' }).click();

  const calls = await page.evaluate(() => (window as any).__CAD_TRANSPILE_CALLS__);
  const providerCall = calls.find((call: any) => call.cmd === 'send_agy_provider_prompt');
  expect(providerCall?.args?.input?.promptText).toContain('Translate the foreign CAD source');
  expect(providerCall?.args?.input?.promptText).toContain(FOREIGN_SOURCE);
  expect(calls.filter((call: any) => call.cmd === 'generate_design')).toHaveLength(0);
  await expect(modal).toBeHidden();
  await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();
});

test('Given Ecky helpers and components precede model When Code opens Then translation is not offered', async ({ page }) => {
  await installMocks(page, 'success');
  const { modal, editor } = await openForeignCode(page);
  const componentModel = [
    '(define (double value) (+ value value))',
    '(define-component bracket ((number width)) (box width 10 4))',
    '(model',
    '  (verify (tag bracket-model) (metric model (manifest has-model-stl)) (expect model (= true)))',
    '  (part body (bracket :width 20)))',
  ].join('\n');

  await editor.click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
  await page.keyboard.insertText(componentModel);

  await expect(editor).toContainText('(define-component bracket');
  await expect(modal.getByRole('button', { name: 'TRANSLATE TO ECKY' })).toHaveCount(0);
  await expect(modal.getByRole('button', { name: 'VERIFY EXISTS' })).toBeVisible();
});

test('Given translation is pending When Code remains open Then action shows pending and original source remains visible', async ({ page }) => {
  await installMocks(page, 'pending');
  const { modal, editor } = await openForeignCode(page);

  await modal.getByRole('button', { name: 'TRANSLATE TO ECKY' }).click({ noWaitAfter: true });
  await expect(modal.getByRole('button', { name: 'TRANSLATING...' })).toBeVisible();
  await expect(editor).toContainText('cube([10, 20, 30])');
  await expect(modal).toBeHidden({ timeout: 5_000 });
  await expect(page.locator('[data-window-id="dialogue"]')).toBeVisible();
});

test('Given AGY translation fails When raw error returns Then original buffer stays recoverable and raw body is visible', async ({ page }) => {
  await installMocks(page, 'error', true);
  const { modal, editor } = await openForeignCode(page);

  await modal.getByRole('button', { name: 'TRANSLATE TO ECKY' }).click();
  await expect(editor).toContainText('cube([10, 20, 30])');
  await expect(modal.locator('.commit-error')).toContainText('AGY request rejected');
  await expect(modal.locator('.commit-error')).toContainText('RAW_AGY_BODY: conversation unavailable');
  await expect(modal.locator('.commit-error')).not.toContainText(/check api key/i);
});
