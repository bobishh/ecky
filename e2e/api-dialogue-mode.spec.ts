import { test, expect, type Page } from '@playwright/test';

function boxStl(name: string) {
  return [
    `solid ${name}`,
    '  facet normal 0 0 0',
    '    outer loop',
    '      vertex 0 0 0',
    '      vertex 10 0 0',
    '      vertex 0 10 0',
    '    endloop',
    '  endfacet',
    'endsolid test',
  ].join('\n');
}

type ApiDialogueMockMode = 'design' | 'questionWithoutFinal';

async function installApiDialogueMocks(
  page: Page,
  mode: ApiDialogueMockMode = 'design',
  options: {
    initialHistory?: any[];
    structuralResult?: any;
    structuralResults?: any[];
    maxGenerationAttempts?: number;
  } = {},
) {
  await page.route(/\/mock\/.*\.stl(\?.*)?$/, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: boxStl('preview'),
    });
  });

  await page.addInitScript(({ mockMode, initialHistory, structuralResult, structuralResults, maxGenerationAttempts }) => {
    const mockWindow = window as any;
    const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    const nowSeconds = () => Math.floor(Date.now() / 1000);

    const bundle = {
      modelId: 'api-dialogue-model',
      sourceKind: 'generated',
      contentHash: 'dialogue-hash',
      fcstdPath: '/mock/output.FCStd',
      manifestPath: '/mock/manifest.json',
      previewStlPath: '/mock/output.stl',
      viewerAssets: [],
    };

    const manifest = {
      modelId: 'api-dialogue-model',
      sourceKind: 'generated',
      document: {
        documentName: 'API Cup',
        documentLabel: 'API Cup',
        objectCount: 1,
        warnings: [],
      },
      parts: [],
      parameterGroups: [],
      selectionTargets: [],
      warnings: [],
      enrichmentState: { status: 'none', proposals: [] },
    };

    mockWindow.__MOCK_THREADS__ = {};
    mockWindow.__MOCK_HISTORY__ = Array.isArray(initialHistory) ? initialHistory : [];
    mockWindow.__MOCK_LAST_DESIGN__ = null;
    mockWindow.__MOCK_PENDING_PROMPT__ = null;
    mockWindow.__MOCK_BUNDLE__ = bundle;
    mockWindow.__MOCK_MANIFEST__ = manifest;
    mockWindow.__MOCK_GENERATE_CALLS__ = [];
    mockWindow.__MOCK_FINALIZE_CALLS__ = [];
    mockWindow.__MOCK_VERIFY_CALLS__ = [];
    mockWindow.__MOCK_MODE__ = mockMode;
    mockWindow.__MOCK_STRUCTURAL_RESULT__ = structuralResult ?? null;
    mockWindow.__MOCK_STRUCTURAL_RESULTS__ = Array.isArray(structuralResults) ? [...structuralResults] : [];

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') {
        return {
          engines: [
            {
              id: 'api-main',
              name: 'API Main',
              provider: 'openai',
              apiKey: 'sk-live',
              model: 'gpt-4.1',
              lightModel: 'gpt-4.1-mini',
              baseUrl: '',
              enabled: true,
            },
          ],
          selectedEngineId: 'api-main',
          freecadCmd: '',
          assets: [],
          microwave: { humId: null, dingId: null, muted: true },
          mcp: {
            port: null,
            maxSessions: null,
            mode: 'passive',
            primaryAgentId: null,
            promptTimeoutSecs: 1800,
            autoAgents: [],
          },
          hasSeenOnboarding: true,
          connectionType: 'api_key',
          defaultEngineKind: 'build123d',
          defaultSourceLanguage: 'build123d',
          defaultGeometryBackend: 'build123d',
          maxGenerationAttempts: maxGenerationAttempts ?? 1,
          maxVerifyAttempts: 0,
        };
      }
      if (cmd === 'save_config') return null;
      if (cmd === 'list_models') return ['gpt-4.1', 'gpt-4.1-mini'];
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: {
            engineKind: 'build123d',
            sourceLanguage: 'build123d',
            geometryBackend: 'build123d',
          },
        };
      }
      if (cmd === 'check_freecad') return true;
      if (cmd === 'get_default_macro') return 'from build123d import *';
      if (cmd === 'get_history') return mockWindow.__MOCK_HISTORY__;
      if (cmd === 'get_last_design') return mockWindow.__MOCK_LAST_DESIGN__;
      if (cmd === 'get_thread') {
        return mockWindow.__MOCK_THREADS__?.[args?.threadId ?? ''] ?? null;
      }
      if (cmd === 'get_thread_messages_page') {
        const thread = mockWindow.__MOCK_HISTORY__.find((item: any) => item.id === args?.threadId) ?? null;
        return {
          messages: thread?.messages ?? [],
          hasMore: false,
          nextBefore: null,
        };
      }
      if (cmd === 'get_thread_latest_version') return null;
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: args?.threadId ?? null,
          connectionState: 'disconnected',
          sessions: [],
          primaryAgentLabel: null,
          statusText: '',
          phase: null,
          busy: false,
          agentLabel: null,
          activityLabel: '',
          sessionId: null,
        };
      }
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      if (cmd === 'init_generation_attempt') {
        mockWindow.__MOCK_PENDING_PROMPT__ = {
          threadId: args.threadId,
          prompt: args.prompt,
        };
        await sleep(1500);
        return 'msg-api-pending';
      }
      if (cmd === 'classify_intent') {
        if (mockWindow.__MOCK_MODE__ === 'questionWithoutFinal') {
          return {
            intentMode: 'question',
            confidence: 0.9,
            response: 'Acknowledging the previous question about the language and hydroponic module.',
            finalResponse: null,
            usage: null,
          };
        }
        return {
          intentMode: 'design',
          confidence: 1,
          response: 'Routing request...',
          finalResponse: null,
          usage: null,
        };
      }
      if (cmd === 'generate_design') {
        const questionMode = Boolean(args?.options?.questionMode ?? args?.questionMode);
        mockWindow.__MOCK_GENERATE_CALLS__.push({ questionMode, prompt: args?.prompt ?? '' });
        await sleep(1200);
        if (questionMode) {
          return {
            threadId: args.threadId,
            messageId: 'msg-api-final',
            design: {
              title: 'API Question',
              versionName: 'Q&A',
              response: 'Full answer: Ecky language is viable; missing pieces are examples and helper syntax, not parsing.',
              interactionMode: 'question',
              macroCode: '',
              macroDialect: 'build123d',
              sourceLanguage: 'build123d',
              geometryBackend: 'build123d',
              uiSpec: { fields: [] },
              initialParams: {},
              postProcessing: null,
            },
            usage: null,
          };
        }
        return {
          threadId: args.threadId,
          messageId: 'msg-api-final',
          design: {
            title: 'API Cup',
            versionName: 'V1',
            response: 'Cup ready.',
            interactionMode: 'design',
            macroCode: 'from build123d import *\nwith BuildPart() as cup:\n    Box(20, 20, 20)\n',
            macroDialect: 'build123d',
            sourceLanguage: 'build123d',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
        };
      }
      if (cmd === 'render_model') return mockWindow.__MOCK_BUNDLE__;
      if (cmd === 'get_model_manifest') return mockWindow.__MOCK_MANIFEST__;
      if (cmd === 'save_model_manifest') return null;
      if (cmd === 'verify_generated_model') {
        mockWindow.__MOCK_VERIFY_CALLS__.push(args);
        if (mockWindow.__MOCK_STRUCTURAL_RESULTS__.length > 0) {
          return mockWindow.__MOCK_STRUCTURAL_RESULTS__.shift();
        }
        if (mockWindow.__MOCK_STRUCTURAL_RESULT__) return mockWindow.__MOCK_STRUCTURAL_RESULT__;
        return {
          passed: true,
          summary: 'Structural checks passed.',
          issues: [],
          metrics: {
            partCount: 1,
            previewStlSizeBytes: 1024,
            totalVolume: 8000,
            totalArea: 2400,
            bbox: null,
          },
          verifierStatus: 'ok',
        };
      }
      if (cmd === 'save_last_design') {
        mockWindow.__MOCK_LAST_DESIGN__ = args.snapshot ?? null;
        return null;
      }
      if (cmd === 'finalize_generation_attempt') {
        mockWindow.__MOCK_FINALIZE_CALLS__.push(args);
        const pending = mockWindow.__MOCK_PENDING_PROMPT__;
        const threadId = pending?.threadId ?? 'thread-api';
        const prompt = pending?.prompt ?? 'missing prompt';
        const succeeded = args.status === 'success';
        const thread = {
          id: threadId,
          title: 'API Cup',
          summary: '',
          updatedAt: nowSeconds(),
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          finalizedAt: null,
          pendingConfirm: null,
          engineKind: 'build123d',
          sourceLanguage: 'build123d',
          geometryBackend: 'build123d',
          messages: [
            {
              id: 'msg-user-final',
              role: 'user',
              content: prompt,
              status: 'success',
              timestamp: nowSeconds() - 1,
            },
            {
              id: args.messageId,
              role: 'assistant',
              content: succeeded ? (args.responseText ?? 'Cup ready.') : (args.errorMessage ?? 'Generation failed.'),
              status: args.status,
              timestamp: nowSeconds(),
              output: args.design,
              artifactBundle: succeeded ? mockWindow.__MOCK_BUNDLE__ : null,
              modelManifest: succeeded ? mockWindow.__MOCK_MANIFEST__ : null,
              usage: null,
            },
          ],
        };
        mockWindow.__MOCK_THREADS__[threadId] = thread;
        mockWindow.__MOCK_HISTORY__ = [thread];
        return null;
      }
      if (cmd === 'update_version_preview') return null;
      if (cmd === 'get_message_attachments') return [];
      return null;
    };
  }, {
    mockMode: mode,
    initialHistory: options.initialHistory ?? [],
    structuralResult: options.structuralResult ?? null,
    structuralResults: options.structuralResults ?? [],
    maxGenerationAttempts: options.maxGenerationAttempts,
  });
}

async function openDialogue(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await expect(page.getByRole('region', { name: 'Prompt panel' })).toBeVisible();
}

test.describe('API dialogue mode', () => {
  test('Given live API engine When app boots Then mascot stays connected', async ({ page }) => {
    await installApiDialogueMocks(page);

    await openDialogue(page);

    await expect(page.locator('.genie-shell')).toHaveAttribute('data-agent-connected', 'true');
    await expect(page.getByRole('button', { name: 'PROCESS' })).toBeVisible();
  });

  test('Given blank API thread When prompt submits Then chat shows user request immediately and pending assistant before persistence', async ({ page }) => {
    await installApiDialogueMocks(page);

    await openDialogue(page);

    const prompt = 'make me a teapot';
    const promptInput = page.getByPlaceholder(/Type a question or design change/i);
    await promptInput.fill(prompt);
    await promptInput.press('Meta+Enter');

    await expect(page.locator('.trail-user')).toContainText(prompt);
    await expect(page.locator('.trail-assistant')).toContainText(/Routing request|Processing request/i);
    await expect(page.getByRole('button', { name: /DONE .*make me a teapot/i })).toBeVisible({
      timeout: 10000,
    });
  });

  test('Given inline reference image in thread When dialogue renders Then message image loads', async ({ page }) => {
    const referenceImage =
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=';
    await installApiDialogueMocks(page, 'design', {
      initialHistory: [
        {
          id: 'thread-inline-reference',
          title: 'Inline Reference',
          summary: '',
          updatedAt: Math.floor(Date.now() / 1000),
          versionCount: 0,
          pendingCount: 1,
          queuedCount: 1,
          errorCount: 0,
          status: 'active',
          finalizedAt: null,
          pendingConfirm: null,
          engineKind: 'build123d',
          sourceLanguage: 'build123d',
          geometryBackend: 'build123d',
          messages: [
            {
              id: 'msg-inline-reference',
              role: 'user',
              content: 'shape like this',
              status: 'pending',
              timestamp: Math.floor(Date.now() / 1000),
              attachmentImages: [referenceImage],
            },
          ],
        },
      ],
    });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).first().click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const image = page.getByAltText('Attached reference image');
    await expect(image).toBeVisible();
    await expect(image).toHaveAttribute('src', referenceImage);
    await expect(image).toHaveJSProperty('naturalWidth', 1);
  });

  test('Given classifier routes question without final answer When prompt submits Then chat persists generated answer', async ({ page }) => {
    await installApiDialogueMocks(page, 'questionWithoutFinal');

    await openDialogue(page);

    const promptInput = page.getByPlaceholder(/Type a question or design change/i);
    await promptInput.fill('are you having issues with the language?');
    await promptInput.press('Meta+Enter');

    await expect(page.locator('.trail-assistant').last()).toContainText(/Answering question|Acknowledging/i);
    await expect(page.locator('.trail-assistant').last()).toContainText('Full answer: Ecky language is viable', {
      timeout: 10000,
    });
    await expect(page.locator('.trail-assistant').last()).not.toContainText('Acknowledging the previous question');

    const generateCalls = await page.evaluate(() => (window as any).__MOCK_GENERATE_CALLS__);
    expect(generateCalls).toEqual([
      expect.objectContaining({ questionMode: true }),
    ]);
  });

  test('Given final structural verification fails When prompt submits Then bad model is not committed as success', async ({ page }) => {
    await installApiDialogueMocks(page, 'design', {
      maxGenerationAttempts: 1,
      structuralResult: {
        passed: false,
        summary: 'Structural verification failed: PREVIEW_STL_DISCONNECTED_COMPONENTS',
        issues: [
          {
            code: 'PREVIEW_STL_DISCONNECTED_COMPONENTS',
            message: 'Preview STL contains 2 disconnected triangle components.',
            partId: null,
            numericPayload: 2,
          },
        ],
        metrics: {
          partCount: 2,
          previewStlSizeBytes: 2048,
          previewStlTriangleCount: 128,
          previewStlComponentCount: 2,
          previewStlNonManifoldEdgeCount: 0,
          previewStlOverhangTriangleCount: 0,
          previewStlOverhangRatio: 0,
          totalVolume: 1000,
          totalArea: 500,
          bbox: null,
        },
        verifierStatus: 'ok',
        verifierSource: 'rustStructural',
      },
    });

    await openDialogue(page);

    const promptInput = page.getByPlaceholder(/Type a question or design change/i);
    await promptInput.fill('make disconnected parts');
    await promptInput.press('Meta+Enter');

    await expect(page.locator('.trail-assistant').last()).toContainText('Structural verification failed', {
      timeout: 10000,
    });

    const finalizeCalls = await page.evaluate(() => (window as any).__MOCK_FINALIZE_CALLS__);
    expect(finalizeCalls.at(-1)).toEqual(expect.objectContaining({
      status: 'error',
      errorMessage: expect.stringContaining('PREVIEW_STL_DISCONNECTED_COMPONENTS'),
    }));
  });

  test('Given final authored verify failure When prompt submits Then failure copy names authored verify and bad model is not committed', async ({ page }) => {
    await installApiDialogueMocks(page, 'design', {
      maxGenerationAttempts: 1,
      structuralResult: {
        passed: false,
        summary: 'Structural verification failed: AUTHORED_VERIFY_FAILED',
        issues: [
          {
            code: 'AUTHORED_VERIFY_FAILED',
            message: 'Expected manifest has-step to equal false, got true.',
            partId: null,
            numericPayload: null,
          },
        ],
        metrics: {
          partCount: 1,
          previewStlSizeBytes: 1024,
          previewStlTriangleCount: 32,
          previewStlComponentCount: 1,
          previewStlNonManifoldEdgeCount: 0,
          previewStlOverhangTriangleCount: 0,
          previewStlOverhangRatio: 0,
          totalVolume: 250,
          totalArea: 140,
          bbox: null,
        },
        verifierStatus: 'ok',
        verifierSource: 'rustStructural',
      },
    });

    await openDialogue(page);

    const promptInput = page.getByPlaceholder(/Type a question or design change/i);
    await promptInput.fill('make box without step export');
    await promptInput.press('Meta+Enter');

    await expect(page.locator('.trail-assistant').last()).toContainText('Authored verify requirements failed.', {
      timeout: 10000,
    });
    await expect(page.locator('.trail-assistant').last()).toContainText('AUTHORED_VERIFY_FAILED');

    const finalizeCalls = await page.evaluate(() => (window as any).__MOCK_FINALIZE_CALLS__);
    expect(finalizeCalls.at(-1)).toEqual(expect.objectContaining({
      status: 'error',
      errorMessage: expect.stringContaining('Authored verify requirements failed.'),
    }));
  });

  test('Given structural verification fails once and passes on retry When prompt submits Then mocked generation finishes green after one repair loop', async ({ page }) => {
    await installApiDialogueMocks(page, 'design', {
      maxGenerationAttempts: 2,
      structuralResults: [
        {
          passed: false,
          summary: 'Structural verification failed: PREVIEW_STL_DISCONNECTED_COMPONENTS',
          issues: [
            {
              code: 'PREVIEW_STL_DISCONNECTED_COMPONENTS',
              message: 'Preview STL contains 2 disconnected triangle components.',
              partId: null,
              numericPayload: 2,
            },
          ],
          metrics: {
            partCount: 2,
            previewStlSizeBytes: 2048,
            previewStlTriangleCount: 128,
            previewStlComponentCount: 2,
            previewStlNonManifoldEdgeCount: 0,
            previewStlOverhangTriangleCount: 0,
            previewStlOverhangRatio: 0,
            totalVolume: 1000,
            totalArea: 500,
            bbox: null,
          },
          verifierStatus: 'ok',
          verifierSource: 'rustStructural',
        },
        {
          passed: true,
          summary: 'Structural checks passed.',
          issues: [],
          metrics: {
            partCount: 1,
            previewStlSizeBytes: 1024,
            previewStlTriangleCount: 32,
            previewStlComponentCount: 1,
            previewStlNonManifoldEdgeCount: 0,
            previewStlOverhangTriangleCount: 0,
            previewStlOverhangRatio: 0,
            totalVolume: 8000,
            totalArea: 2400,
            bbox: null,
          },
          verifierStatus: 'ok',
          verifierSource: 'rustStructural',
        },
      ],
    });

    await openDialogue(page);

    const promptInput = page.getByPlaceholder(/Type a question or design change/i);
    await promptInput.fill('make one sealed cup');
    await promptInput.press('Meta+Enter');

    await expect(page.getByRole('button', { name: /DONE .*make one sealed cup/i })).toBeVisible({
      timeout: 15000,
    });
    await expect(page.locator('.trail-assistant').last()).toContainText('Cup ready.');
    await expect(page.locator('.trail-assistant').last()).not.toContainText('Structural verification failed');

    const generateCalls = await page.evaluate(() => (window as any).__MOCK_GENERATE_CALLS__);
    expect(generateCalls).toHaveLength(2);
    expect(generateCalls[1]?.prompt).toContain('Structural verification failed');
    expect(generateCalls[1]?.prompt).toContain('Please fix the geometry code to resolve the structural issues.');

    const verifyCalls = await page.evaluate(() => (window as any).__MOCK_VERIFY_CALLS__);
    expect(verifyCalls).toHaveLength(2);

    const finalizeCalls = await page.evaluate(() => (window as any).__MOCK_FINALIZE_CALLS__);
    expect(finalizeCalls).toHaveLength(1);
    expect(finalizeCalls[0]).toEqual(expect.objectContaining({
      status: 'success',
    }));
  });
});
