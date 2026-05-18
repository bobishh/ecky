import { expect, test, type Page } from '@playwright/test';
import os from 'node:os';
import path from 'node:path';
import { promises as fs } from 'node:fs';

type MockOptions = {
  queueFails?: boolean;
  queueDelayMs?: number;
  queuedCount?: number;
  pendingConfirm?: string | null;
  messages?: unknown[];
  errorCount?: number;
  runtimeFilesExist?: boolean;
  allowRuntimeRebuild?: boolean;
  messagesPageDelayMs?: number;
  latestVersionDelayMs?: number;
};

async function installPassiveThreadAgentMock(page: Page, options: MockOptions = {}) {
  await page.addInitScript((mockOptions: MockOptions) => {
    const now = Math.floor(Date.now() / 1000);
    const calls = { queue: 0, generate: 0 };
    const invokeCalls: Array<{ cmd: string; args: unknown }> = [];
    let mockMessages = [...(mockOptions.messages ?? [])];
    const latestRenderableVersion = () =>
      [...mockMessages]
        .reverse()
        .find((message: any) => message?.role === 'assistant' && message?.status === 'success' && message?.artifactBundle) ?? null;
    const latestVersion: any = latestRenderableVersion();
    const rebuiltArtifactBundle = latestVersion?.artifactBundle
      ? {
          ...latestVersion.artifactBundle,
          modelId: `${latestVersion.artifactBundle.modelId}-rebuilt`,
          contentHash: `${latestVersion.artifactBundle.contentHash}-rebuilt`,
          previewStlPath: '/mock/rebuilt-preview.stl',
        }
      : null;
    const rebuiltModelManifest = latestVersion?.modelManifest
      ? {
          ...latestVersion.modelManifest,
          modelId: rebuiltArtifactBundle?.modelId ?? latestVersion.modelManifest.modelId,
        }
      : null;
    const thread = {
      id: 'thread-1',
      title: 'Passive MCP Thread',
      summary: 'Thread controlled by external agent.',
      messages: mockMessages,
      updatedAt: now,
      versionCount: 0,
      pendingCount: 0,
      queuedCount: mockOptions.queuedCount ?? 0,
      errorCount: mockOptions.errorCount ?? 0,
      genieTraits: null,
      status: 'active',
      finalizedAt: null,
      pendingConfirm: mockOptions.pendingConfirm ?? null,
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'build123d',
    };

    (window as Window & typeof globalThis & {
      __MOCK_AGENT_DIALOGUE_CALLS__?: typeof calls;
      __MOCK_AGENT_INVOKE_CALLS__?: typeof invokeCalls;
    }).__MOCK_AGENT_DIALOGUE_CALLS__ = calls;
    (window as Window & typeof globalThis & {
      __MOCK_AGENT_INVOKE_CALLS__?: typeof invokeCalls;
    }).__MOCK_AGENT_INVOKE_CALLS__ = invokeCalls;

    const currentThread = () => ({
      ...thread,
      messages: mockMessages,
      versionCount: mockMessages.filter((message: any) =>
        message?.role === 'assistant' &&
        message?.status === 'success' &&
        message?.artifactBundle
      ).length,
    });

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      invokeCalls.push({ cmd, args });
      if (cmd === 'get_config') {
        return {
          engines: [],
          selectedEngineId: '',
          hasSeenOnboarding: true,
          freecadCmd: '',
          assets: [],
          microwave: null,
          mcp: {
            port: null,
            maxSessions: null,
            mode: 'passive',
            primaryAgentId: null,
            promptTimeoutSecs: 1800,
            autoAgents: [],
          },
          connectionType: null,
          defaultEngineKind: 'ecky',
          defaultSourceLanguage: 'ecky',
          defaultGeometryBackend: 'build123d',
          maxGenerationAttempts: 3,
          maxVerifyAttempts: 0,
        };
      }
      if (cmd === 'save_config') return null;
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: false, detail: 'missing', path: null },
          build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: {
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
          },
        };
      }
      if (cmd === 'check_freecad') return false;
      if (cmd === 'get_default_macro') return '(model)';
      if (cmd === 'get_history') return [currentThread()];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_thread') return currentThread();
      if (cmd === 'get_thread_latest_version') {
        if (mockOptions.latestVersionDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, mockOptions.latestVersionDelayMs));
        }
        return latestRenderableVersion();
      }
      if (cmd === 'get_thread_message_version') {
        return mockMessages.find((message: any) => message?.id === args?.messageId) ?? null;
      }
      if (cmd === 'get_thread_messages_page') {
        if (mockOptions.messagesPageDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, mockOptions.messagesPageDelayMs));
        }
        return { messages: mockMessages, nextBefore: null, hasMore: false };
      }
      if (cmd === 'delete_version') {
        mockMessages = mockMessages.filter((message: any) => message?.id !== args?.messageId);
        return null;
      }
      if (cmd === 'get_active_agent_sessions') {
        return [
          {
            sessionId: 'sess-1',
            clientKind: 'mcp-http',
            hostLabel: 'Gemini CLI',
            agentLabel: 'Gemini',
            llmModelId: null,
            llmModelLabel: null,
            threadId: 'thread-1',
            messageId: null,
            modelId: null,
            phase: 'waiting_for_user',
            statusText: 'Waiting for your next message...',
            updatedAt: now,
          },
        ];
      }
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: args?.threadId ?? null,
          connectionState: 'waiting',
          agentLabel: 'Gemini',
          llmModelLabel: null,
          providerKind: 'gemini-cli',
          sessionId: 'sess-1',
          phase: 'waiting_for_user',
          statusText: 'Waiting for your next message...',
          busy: false,
          activityLabel: null,
          activityStartedAt: null,
          attentionKind: null,
          waitingOnPrompt: false,
          updatedAt: now,
        };
      }
      if (cmd === 'prepare_prompt_workspace_capture') {
        return {
          path: '/mock/workspace-annotated.png',
          name: 'workspace-annotated.png',
          explanation: 'Current workspace view with hand-drawn annotations.',
          kind: 'image',
        };
      }
      if (cmd === 'prepare_prompt_attachments') {
        return args?.attachments ?? [];
      }
      if (cmd === 'queue_agent_prompt') {
        calls.queue += 1;
        if (mockOptions.queueDelayMs) {
          await new Promise((resolve) => setTimeout(resolve, mockOptions.queueDelayMs));
        }
        if (mockOptions.queueFails) {
          throw new Error('queue exploded');
        }
        return { threadId: 'thread-1', messageId: 'queued-1' };
      }
      if (cmd === 'generate_design') {
        calls.generate += 1;
        throw new Error('generate path should stay unused');
      }
      if (cmd === 'plugin:fs|exists') {
        if ((args as any)?.path === '/mock/rebuilt-preview.stl') return true;
        return mockOptions.runtimeFilesExist ?? true;
      }
      if (cmd === 'plugin:fs|size') return 1024;
      if (cmd === 'render_model' && mockOptions.allowRuntimeRebuild) {
        return rebuiltArtifactBundle;
      }
      if (cmd === 'get_model_manifest' && mockOptions.allowRuntimeRebuild) {
        return rebuiltModelManifest;
      }
      if (cmd === 'update_version_runtime') return null;
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      return null;
    };
  }, options);
}

async function installMockStlRoutes(page: Page) {
  await page.route(/\/mock\/.*\.stl(?:\?.*)?$/, async (route) => {
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
}

async function writeMockStlFile(name: string): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'ecky-dialogue-stl-'));
  const stlPath = path.join(dir, `${name}.stl`);
  await fs.writeFile(
    stlPath,
    `solid mock
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid mock
`,
    'utf8',
  );
  return stlPath;
}

async function drawViewportAnnotation(page: Page) {
  await page.getByTitle('Draw Annotations').click();
  await expect(page.getByTitle('Exit Draw Mode')).toBeVisible();
  const canvas = page.locator('.drawing-canvas');
  await expect(canvas).toBeVisible();
  const box = await canvas.boundingBox();
  if (!box) {
    throw new Error('Drawing canvas missing bounding box');
  }
  const startX = box.x + box.width * 0.22;
  const startY = box.y + box.height * 0.38;
  const endX = startX + 70;
  const endY = startY + 40;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(endX, endY);
  await page.mouse.up();
}

test.describe('Dialogue routes passive thread-owned MCP threads through queue mode', () => {
  test.beforeEach(async ({ page }) => {
    await installMockStlRoutes(page);
  });

  test('Given concept preview image When opening loupe Then image opens read-only without switching the viewport target', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    const conceptSvg = [
      '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180" viewBox="0 0 320 180">',
      '<rect width="320" height="180" fill="#101321"/>',
      '<path d="M40 90 H280" stroke="#45d7ff" stroke-width="10"/>',
      '<text x="42" y="55" fill="#d6a94a" font-family="monospace" font-size="18">concept svg</text>',
      '</svg>',
    ].join('');
    await installPassiveThreadAgentMock(page, {
      messages: [
        {
          id: 'concept-preview-1',
          role: 'assistant',
          content: 'Side trap sketch',
          status: 'success',
          output: null,
          usage: null,
          artifactBundle: null,
          modelManifest: null,
          agentOrigin: null,
          imageData: `data:image/svg+xml;base64,${btoa(conceptSvg)}`,
          visualKind: 'conceptPreview',
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click({ force: true });

    await expect(page.locator('.trail-image-kicker')).toContainText('CONCEPT');
    await expect(page.getByRole('button', { name: 'OPEN IN VIEWPORT' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'LOUPE' })).toHaveCount(0);
    await page.locator('.trail-image').click();

    const loupe = page.getByRole('dialog', { name: /CONCEPT/i });
    await expect(loupe).toBeVisible();
    await expect(loupe.locator('.visual-loupe__image')).toHaveAttribute('src', /data:image\/svg\+xml;base64,/);
    await expect(loupe).toContainText('Side trap sketch');
  });

  test('Given version history item When dialogue opens Then actions are set-current, view, code, delete without copy', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    const versionStlPath = await writeMockStlFile('version-preview');
    await installPassiveThreadAgentMock(page, {
      messages: [
        {
          id: 'version-1',
          role: 'assistant',
          content: 'Parameter apply committed as new version.',
          status: 'success',
          output: {
            title: 'Cached Bracket',
            versionName: 'V-param',
            interactionMode: 'design',
            macroCode: '(model)',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'version-model-1',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'hash-version-1',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/version-manifest.json',
            macroPath: '/mock/version.ecky',
            previewStlPath: versionStlPath,
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'version-model-1',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Cached Bracket',
              documentLabel: 'Cached Bracket',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    const dialogueWindow = page.locator('[data-window-id="dialogue"]');
    await expect(dialogueWindow).toBeHidden();
    await page.locator('button[title="Dialogue"]').evaluate((button) => (button as HTMLButtonElement).click());
    await expect(dialogueWindow).toBeVisible();

    const versionItem = page.locator('.trail-item').filter({ hasText: 'Cached Bracket' });
    await expect(versionItem.getByRole('button', { name: /SET CURRENT|CURRENT/ })).toBeVisible();
    await expect(versionItem.getByRole('button', { name: 'VIEW' })).toBeVisible();
    await expect(versionItem.getByRole('button', { name: 'CODE' })).toBeVisible();
    await expect(versionItem.getByRole('button', { name: 'COPY' })).toHaveCount(0);
    await expect(versionItem.getByRole('button', { name: 'OPEN' })).toHaveCount(0);

    await versionItem.getByRole('button', { name: 'VIEW' }).click();
    const modal = page.getByRole('dialog', { name: /VERSION PREVIEW/i });
    await expect(modal).toBeVisible();
    await expect(modal).toContainText('Cached Bracket');
    await expect(modal.locator('.version-loupe__viewer')).toBeVisible();

    const beforeDragBox = await page.locator('[data-window-id="dialogue"]').boundingBox();
    const viewerBox = await modal.locator('.version-loupe__viewer').boundingBox();
    if (!beforeDragBox || !viewerBox) throw new Error('version preview drag test missing boxes');
    await page.mouse.move(viewerBox.x + viewerBox.width / 2, viewerBox.y + viewerBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(viewerBox.x + viewerBox.width / 2 + 80, viewerBox.y + viewerBox.height / 2 + 30);
    await page.mouse.up();

    const afterDragBox = await page.locator('[data-window-id="dialogue"]').boundingBox();
    expect(afterDragBox?.x).toBe(beforeDragBox.x);
    expect(afterDragBox?.y).toBe(beforeDragBox.y);
  });

  test('Given saved version preview When switching current version Then switch does not rewrite project preview', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    const versionMessage = (id: string, versionName: string, timestamp: number) => ({
      id,
      role: 'assistant',
      content: `${versionName} ready.`,
      status: 'success',
      output: {
        title: 'Saved Bracket',
        versionName,
        interactionMode: 'design',
        macroCode: '(model)',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      },
      usage: null,
      artifactBundle: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        contentHash: `hash-${id}`,
        artifactVersion: 1,
        fcstdPath: '',
        manifestPath: `/mock/${id}-manifest.json`,
        macroPath: `/mock/${id}.ecky`,
        previewStlPath: `/mock/${id}.stl`,
        viewerAssets: [],
      },
      modelManifest: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        document: {
          documentName: 'Saved Bracket',
          documentLabel: 'Saved Bracket',
          objectCount: 1,
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
      },
      agentOrigin: null,
      imageData: `data:image/png;base64,${btoa(id)}`,
      visualKind: null,
      attachmentImages: [],
      timestamp,
    });

    await installPassiveThreadAgentMock(page, {
      latestVersionDelayMs: 1500,
      messages: [
        versionMessage('version-1', 'V-1', now - 2),
        versionMessage('version-2', 'V-2', now - 1),
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const versionOne = page.locator('.trail-item').filter({ hasText: 'V-1' });
    await versionOne.getByRole('button', { name: 'SET CURRENT' }).click();
    await expect(versionOne.getByRole('button', { name: 'CURRENT' })).toBeVisible();

    await page.waitForTimeout(250);

    const previewUpdates = await page.evaluate(() =>
      ((window as Window & typeof globalThis & {
        __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: unknown }>;
      }).__MOCK_AGENT_INVOKE_CALLS__ ?? []).filter((call) => call.cmd === 'update_version_preview').length,
    );
    expect(previewUpdates).toBe(0);
  });

  test('Given version thumbnail update When switching current version Then viewport model selection stays on selected render key', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    const versionMessage = (id: string, versionName: string, timestamp: number) => ({
      id,
      role: 'assistant',
      content: `${versionName} ready.`,
      status: 'success',
      output: {
        title: 'Render Key Guard',
        versionName,
        interactionMode: 'design',
        macroCode: '(model)',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      },
      usage: null,
      artifactBundle: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        contentHash: `hash-${id}`,
        artifactVersion: 1,
        fcstdPath: '',
        manifestPath: `/mock/${id}-manifest.json`,
        macroPath: `/mock/${id}.ecky`,
        previewStlPath: `/mock/${id}.stl`,
        viewerAssets: [],
      },
      modelManifest: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        document: {
          documentName: 'Render Key Guard',
          documentLabel: 'Render Key Guard',
          objectCount: 1,
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
      },
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp,
    });

    await installPassiveThreadAgentMock(page, {
      messages: [
        versionMessage('version-1', 'V-1', now - 2),
        versionMessage('version-2', 'V-2', now - 1),
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();

    const versionOne = page.locator('.trail-item').filter({ hasText: 'V-1' });
    const setCurrentVersionOne = versionOne.getByRole('button', { name: 'SET CURRENT' });
    if (await setCurrentVersionOne.count()) {
      await setCurrentVersionOne.click();
    }
    await expect(versionOne.getByRole('button', { name: 'CURRENT' })).toBeVisible();

    const invokeCallCounts = await page.evaluate(() => {
      const calls =
        (window as Window & typeof globalThis & {
          __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: unknown }>;
        }).__MOCK_AGENT_INVOKE_CALLS__ ?? [];
      return {
        exists: calls.filter((call) => call.cmd === 'plugin:fs|exists').length,
        renderModel: calls.filter((call) => call.cmd === 'render_model').length,
      };
    });

    await page.evaluate(() => {
      window.dispatchEvent(
        new CustomEvent('ecky:version-preview-updated', {
          detail: {
            threadId: 'thread-1',
            messageId: 'version-2',
            imageData: 'data:image/png;base64,refresh',
          },
        }),
      );
    });

    await page.waitForTimeout(400);
    await expect(versionOne.getByRole('button', { name: 'CURRENT' })).toBeVisible();
    const invokeCallCountsAfterUpdate = await page.evaluate(() => {
      const calls =
        (window as Window & typeof globalThis & {
          __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: unknown }>;
        }).__MOCK_AGENT_INVOKE_CALLS__ ?? [];
      return {
        exists: calls.filter((call) => call.cmd === 'plugin:fs|exists').length,
        renderModel: calls.filter((call) => call.cmd === 'render_model').length,
      };
    });
    expect(invokeCallCountsAfterUpdate).toEqual(invokeCallCounts);
  });

  test('Given live apply is pending When version switches Then stale params do not render on new source', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    const versionMessage = (id: string, versionName: string, width: number, timestamp: number) => ({
      id,
      role: 'assistant',
      content: `${versionName} ready.`,
      status: 'success',
      output: {
        title: 'Live Apply Guard',
        versionName,
        interactionMode: 'design',
        macroCode: `print("${versionName}")`,
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        uiSpec: { fields: [{ type: 'number', key: 'width', label: 'Width' }] },
        initialParams: { width },
        postProcessing: null,
      },
      usage: null,
      artifactBundle: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        contentHash: `hash-${id}`,
        artifactVersion: 1,
        fcstdPath: '',
        manifestPath: `/mock/${id}-manifest.json`,
        macroPath: `/mock/${id}.ecky`,
        previewStlPath: `/mock/live-${id}.stl`,
        viewerAssets: [],
      },
      modelManifest: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        document: {
          documentName: 'Live Apply Guard',
          documentLabel: 'Live Apply Guard',
          objectCount: 1,
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
      },
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp,
    });

    await installMockStlRoutes(page);
    await installPassiveThreadAgentMock(page, {
      allowRuntimeRebuild: true,
      messages: [
        versionMessage('version-1', 'V-1', 10, now - 2),
        versionMessage('version-2', 'V-2', 200, now - 1),
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const versionOne = page.locator('.trail-item').filter({ hasText: 'V-1' });
    const versionTwo = page.locator('.trail-item').filter({ hasText: 'V-2' });
    const setCurrentVersionOne = versionOne.getByRole('button', { name: 'SET CURRENT' });
    if (await setCurrentVersionOne.count()) {
      await setCurrentVersionOne.click();
    }
    await expect(versionOne.getByRole('button', { name: 'CURRENT' })).toBeVisible();

    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    await page.locator('.live-toggle').click();

    const width = page.locator('[data-param-key="width"] input[type="number"]').first();
    await expect(width).toHaveValue('10');
    await width.fill('42');
    await versionTwo.getByRole('button', { name: 'SET CURRENT' }).click();
    await expect(versionTwo.getByRole('button', { name: 'CURRENT' })).toBeVisible();
    await expect(width).toHaveValue('200');

    await page.waitForTimeout(350);
    const staleRender = await page.evaluate(() => {
      const calls =
        (window as Window & typeof globalThis & {
          __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: any }>;
        }).__MOCK_AGENT_INVOKE_CALLS__ ?? [];
      return calls.find(
        (call) =>
          call.cmd === 'render_model' &&
          call.args?.macroCode === 'print("V-2")' &&
          call.args?.parameters?.width === 42,
      ) ?? null;
    });
    expect(staleRender).toBeNull();
  });

  test('Given current version is removed When previous version becomes current Then viewport loads previous version runtime', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    const stlRequests: string[] = [];
    page.on('request', (request) => {
      const url = request.url();
      if (url.includes('/mock/delete-switch-version-')) stlRequests.push(url);
    });
    const versionMessage = (id: string, versionName: string, timestamp: number) => ({
      id,
      role: 'assistant',
      content: `${versionName} ready.`,
      status: 'success',
      output: {
        title: 'Delete Switch Guard',
        versionName,
        interactionMode: 'design',
        macroCode: '(model)',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      },
      usage: null,
      artifactBundle: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        contentHash: `hash-${id}`,
        artifactVersion: 1,
        fcstdPath: '',
        manifestPath: `/mock/${id}-manifest.json`,
        macroPath: `/mock/${id}.ecky`,
        previewStlPath: `/mock/delete-switch-${id}.stl`,
        viewerAssets: [],
      },
      modelManifest: {
        modelId: `model-${id}`,
        sourceKind: 'generated',
        sourceLanguage: 'ecky',
        geometryBackend: 'build123d',
        document: {
          documentName: 'Delete Switch Guard',
          documentLabel: 'Delete Switch Guard',
          objectCount: 1,
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
      },
      agentOrigin: null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp,
    });

    await installPassiveThreadAgentMock(page, {
      messages: [
        versionMessage('version-1', 'V-1', now - 2),
        versionMessage('version-2', 'V-2', now - 1),
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    const visibleViewer = page.locator('.viewer-shell').first();
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
    await expect.poll(() => stlRequests.some((url) => url.includes('delete-switch-version-2.stl'))).toBe(true);
    await expect(visibleViewer).toHaveAttribute('data-stl-url', /delete-switch-version-2\.stl/);

    const versionOne = page.locator('.trail-item').filter({ hasText: 'V-1' });
    const versionTwo = page.locator('.trail-item').filter({ hasText: 'V-2' });
    await expect(versionTwo.getByRole('button', { name: 'CURRENT' })).toBeVisible();

    await page.locator('.version-nav__actions .delete-btn').click();
    await page.getByRole('dialog', { name: /Remove From Carousel/i }).getByRole('button', { name: 'REMOVE' }).click();

    await expect(visibleViewer).not.toHaveAttribute('data-stl-url', /delete-switch-version-2\.stl/, { timeout: 500 });
    await expect(versionOne.getByRole('button', { name: 'CURRENT' })).toBeVisible();
    await expect.poll(() => stlRequests.some((url) => url.includes('delete-switch-version-1.stl'))).toBe(true);
    await expect(visibleViewer).toHaveAttribute('data-stl-url', /delete-switch-version-1\.stl/);
  });

  test('Given version runtime is missing When view opens preview Then loupe rebuilds runtime instead of showing empty artifact state', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    await installPassiveThreadAgentMock(page, {
      runtimeFilesExist: false,
      allowRuntimeRebuild: true,
      messages: [
        {
          id: 'version-missing',
          role: 'assistant',
          content: 'Rebuild me.',
          status: 'success',
          output: {
            title: 'Thomas Modular Ramp Ecky IR',
            versionName: 'V-missing',
            interactionMode: 'design',
            macroCode: '(model)',
            macroDialect: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'missing-model',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'missing-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/missing.json',
            macroPath: '/mock/missing.ecky',
            previewStlPath: '/mock/missing-preview.stl',
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'missing-model',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Thomas Modular Ramp Ecky IR',
              documentLabel: 'Thomas Modular Ramp Ecky IR',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 2,
        },
        {
          id: 'version-current',
          role: 'assistant',
          content: 'Current version.',
          status: 'success',
          output: {
            title: 'Current Renderable Version',
            versionName: 'V-current',
            interactionMode: 'design',
            macroCode: '(model)',
            macroDialect: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'current-model',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'current-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/current.json',
            macroPath: '/mock/current.ecky',
            previewStlPath: '/mock/rebuilt-preview.stl',
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'current-model',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Current Renderable Version',
              documentLabel: 'Current Renderable Version',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 2,
        },
        {
          id: 'version-current-loupe',
          role: 'assistant',
          content: 'Current loupe anchor.',
          status: 'success',
          output: {
            title: 'Current Renderable Version',
            versionName: 'V-current',
            interactionMode: 'design',
            macroCode: '(model)',
            macroDialect: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'current-loupe-model',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'current-loupe-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/current-loupe.json',
            macroPath: '/mock/current-loupe.ecky',
            previewStlPath: '/mock/rebuilt-preview.stl',
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'current-loupe-model',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Current Renderable Version',
              documentLabel: 'Current Renderable Version',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const versionItem = page.locator('.trail-item').filter({ hasText: 'Thomas Modular Ramp Ecky IR' });
    await versionItem.getByRole('button', { name: 'VIEW' }).click();

    const modal = page.getByRole('dialog', { name: /VERSION PREVIEW/i });
    await expect(modal.locator('.version-loupe__viewer')).toBeVisible();
    await expect(modal).not.toContainText('NO RUNTIME ARTIFACT');

    const calls = await page.evaluate(() =>
      ((window as Window & typeof globalThis & {
        __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: unknown }>;
      }).__MOCK_AGENT_INVOKE_CALLS__ ?? []).map((call) => call.cmd),
    );
    expect(calls).toContain('render_model');
    expect(calls).toContain('update_version_runtime');
  });

  test('Given thread messages are still backfilling When project opens Then current version and viewport appear before history finishes', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    await installPassiveThreadAgentMock(page, {
      messagesPageDelayMs: 8000,
      messages: [
        {
          id: 'version-current-fast-open',
          role: 'assistant',
          content: 'Current version visible before history page completes.',
          status: 'success',
          output: {
            title: 'Fast Open Thread',
            versionName: 'V-current-fast-open',
            interactionMode: 'design',
            macroCode: '(model)',
            macroDialect: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'current-fast-open-model',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'current-fast-open-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/current-fast-open.json',
            macroPath: '/mock/current-fast-open.ecky',
            previewStlPath: '/mock/current-fast-open-preview.stl',
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'current-fast-open-model',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Fast Open Thread',
              documentLabel: 'Fast Open Thread',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await expect(page.locator('.thread-loading')).toContainText('LOADING THREAD MESSAGES');

    const versionItem = page.locator('.trail-item').filter({ hasText: 'Fast Open Thread' });
    await expect(versionItem.getByRole('button', { name: 'CURRENT' })).toBeVisible();
    await expect(versionItem.getByRole('button', { name: 'CODE' })).toBeVisible();
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  });

  test('Given projects prefetched latest version When open thread has slow latest and page calls Then dialogue still boots from cache', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    await installPassiveThreadAgentMock(page, {
      latestVersionDelayMs: 1200,
      messagesPageDelayMs: 2000,
      messages: [
        {
          id: 'version-prefetched-open',
          role: 'assistant',
          content: 'Prefetched current version.',
          status: 'success',
          output: {
            title: 'Prefetched Open Thread',
            versionName: 'V-prefetched-open',
            interactionMode: 'design',
            macroCode: '(model)',
            macroDialect: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'prefetched-open-model',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'prefetched-open-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/prefetched-open.json',
            macroPath: '/mock/prefetched-open.ecky',
            previewStlPath: '/mock/prefetched-open-preview.stl',
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'prefetched-open-model',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Prefetched Open Thread',
              documentLabel: 'Prefetched Open Thread',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.waitForTimeout(1300);
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const versionItem = page.locator('.trail-item').filter({ hasText: 'Prefetched Open Thread' });
    await expect(versionItem.getByRole('button', { name: 'CURRENT' })).toBeVisible();
    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
  });

  test('Given current version runtime is missing When project opens Then current model rebuilds instead of staying empty', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    await installPassiveThreadAgentMock(page, {
      runtimeFilesExist: false,
      allowRuntimeRebuild: true,
      messages: [
        {
          id: 'version-missing-current',
          role: 'assistant',
          content: 'Rebuild current.',
          status: 'success',
          output: {
            title: 'Thomas Modular Ramp Ecky IR',
            versionName: 'V-missing-current',
            interactionMode: 'design',
            macroCode: '(model)',
            macroDialect: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            uiSpec: { fields: [] },
            initialParams: {},
            postProcessing: null,
          },
          usage: null,
          artifactBundle: {
            modelId: 'missing-current-model',
            sourceKind: 'generated',
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            contentHash: 'missing-current-hash',
            artifactVersion: 1,
            fcstdPath: '',
            manifestPath: '/mock/missing-current.json',
            macroPath: '/mock/missing-current.ecky',
            previewStlPath: '/mock/missing-current-preview.stl',
            viewerAssets: [],
          },
          modelManifest: {
            modelId: 'missing-current-model',
            sourceKind: 'generated',
            sourceLanguage: 'ecky',
            geometryBackend: 'build123d',
            document: {
              documentName: 'Thomas Modular Ramp Ecky IR',
              documentLabel: 'Thomas Modular Ramp Ecky IR',
              objectCount: 1,
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
          },
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();

    await expect(page.locator('.viewer-shell canvas')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Dismiss error' })).toHaveCount(0);

    const calls = await page.evaluate(() =>
      ((window as Window & typeof globalThis & {
        __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: unknown }>;
      }).__MOCK_AGENT_INVOKE_CALLS__ ?? []).map((call) => call.cmd),
    );
    expect(calls).toContain('render_model');
    expect(calls).toContain('update_version_runtime');
  });

  test('Given recoverable project delete When confirm opens Then copy says trash and actions use themed buttons', async ({ page }) => {
    await installPassiveThreadAgentMock(page, {
      messages: [],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'DELETE' }).click();

    const modal = page.getByRole('dialog', { name: /TRASH PROJECT/i });
    await expect(modal).toBeVisible();
    await expect(modal).toContainText('Move Passive MCP Thread to trash?');
    await expect(modal).toContainText('recover it from TRASH');
    await expect(modal).not.toContainText('DELETE FOREVER');

    const cancelButton = modal.getByRole('button', { name: 'CANCEL' });
    const trashButton = modal.getByRole('button', { name: 'MOVE TO TRASH' });
    await expect(cancelButton).toBeVisible();
    await expect(trashButton).toBeVisible();
    await expect(cancelButton).toHaveClass(/btn/);
    await expect(trashButton).toHaveClass(/btn/);

    const styles = await Promise.all([
      cancelButton.evaluate((node) => {
        const css = window.getComputedStyle(node);
        return {
          background: css.backgroundColor,
          border: css.borderTopColor,
        };
      }),
      trashButton.evaluate((node) => {
        const css = window.getComputedStyle(node);
        return {
          background: css.backgroundColor,
          border: css.borderTopColor,
        };
      }),
    ]);
    expect(styles[0].background).not.toBe('rgb(239, 239, 239)');
    expect(styles[1].background).not.toBe('rgb(239, 239, 239)');

    await trashButton.click();
    await expect(modal).toBeHidden();
    await expect(page.getByText('Passive MCP Thread', { exact: true })).toHaveCount(0);
  });

  test('Given agent tool errors in thread history When dialogue opens Then iteration errors stay out of visible history', async ({ page }) => {
    const now = Math.floor(Date.now() / 1000);
    await installPassiveThreadAgentMock(page, {
      errorCount: 1,
      messages: [
        {
          id: 'user-visible',
          role: 'user',
          content: 'make roof pins',
          status: 'success',
          output: null,
          usage: null,
          artifactBundle: null,
          modelManifest: null,
          agentOrigin: null,
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 2,
        },
        {
          id: 'agent-error-hidden',
          role: 'assistant',
          content: 'Expected a symbolic head for runtime list expression.',
          status: 'error',
          output: null,
          usage: null,
          artifactBundle: null,
          modelManifest: null,
          agentOrigin: {
            sessionId: 'sess-1',
            clientKind: 'mcp-http',
            hostLabel: 'Codex MCP Client',
            agentLabel: 'Ecky',
            llmModelId: null,
            llmModelLabel: null,
            createdAt: now - 1,
          },
          imageData: null,
          visualKind: null,
          attachmentImages: [],
          timestamp: now - 1,
        },
      ],
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    await expect(page.locator('.trail-user')).toContainText('make roof pins');
    await expect(page.locator('.trail-error')).toHaveCount(0);
    await expect(page.locator('.trail-list')).not.toContainText(
      'Expected a symbolic head for runtime list expression.',
    );
  });

  test('Given passive config and external thread agent When selecting thread Then dialogue queues instead of generating', async ({ page }) => {
    await installPassiveThreadAgentMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.locator('.prompt-input').fill('Message for external agent');

    await expect(page.getByRole('button', { name: 'QUEUE' })).toBeVisible();
    await page.locator('.prompt-input').press('Meta+Enter');

    await expect.poll(async () =>
      page.evaluate(
        () =>
          (window as Window & typeof globalThis & {
            __MOCK_AGENT_DIALOGUE_CALLS__?: { queue: number; generate: number };
          }).__MOCK_AGENT_DIALOGUE_CALLS__,
      ),
    ).toMatchObject({ queue: 1, generate: 0 });
  });

  test('Given passive queue is slow When sending message Then user bubble appears before backend refresh', async ({ page }) => {
    await installPassiveThreadAgentMock(page, { queueDelayMs: 3000, messagesPageDelayMs: 3000 });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const prompt = 'Message should paint immediately';
    await page.locator('.prompt-input').fill(prompt);
    await page.locator('.prompt-input').press('Meta+Enter');

    await expect(page.locator('.trail-user')).toContainText(prompt, { timeout: 500 });
    await expect
      .poll(async () =>
        page.evaluate(
          () =>
            (window as Window & typeof globalThis & {
              __MOCK_AGENT_DIALOGUE_CALLS__?: { queue: number; generate: number };
            }).__MOCK_AGENT_DIALOGUE_CALLS__,
        ),
      )
      .toMatchObject({ queue: 1, generate: 0 });
  });

  test('Given planner analyze recipes prompt When queued in passive MCP thread Then flow stays preview-only without auto-commit until explicit apply', async ({ page }) => {
    await installPassiveThreadAgentMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    await page.locator('.prompt-input').fill('planner: run printability analyze + recipes preview only; no commit');
    await page.locator('.prompt-input').press('Meta+Enter');

    await expect(page.locator('.trail-user')).toContainText(
      'planner: run printability analyze + recipes preview only; no commit',
    );
    await expect.poll(async () =>
      page.evaluate(
        () =>
          (window as Window & typeof globalThis & {
            __MOCK_AGENT_DIALOGUE_CALLS__?: { queue: number; generate: number };
          }).__MOCK_AGENT_DIALOGUE_CALLS__,
      ),
    ).toMatchObject({ queue: 1, generate: 0 });

    const commitLikeCalls = await page.evaluate(() =>
      ((window as Window & typeof globalThis & {
        __MOCK_AGENT_INVOKE_CALLS__?: Array<{ cmd: string; args: unknown }>;
      }).__MOCK_AGENT_INVOKE_CALLS__ ?? [])
        .map((call) => call.cmd)
        .filter((cmd) =>
          ['add_manual_version', 'apply_imported_model', 'update_version_runtime', 'update_version_preview'].includes(cmd),
        ),
    );
    expect(commitLikeCalls).toEqual([]);
  });

  test('Given thread messages are still loading When blank project starts Then empty dialogue is not blocked', async ({ page }) => {
    await installPassiveThreadAgentMock(page, { latestVersionDelayMs: 10_000 });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await expect(page.locator('.thread-loading')).toContainText('LOADING THREAD MESSAGES');

    await page.locator('button[title="New project"]').click();
    await page.getByRole('button', { name: 'Blank Project' }).click();

    await expect(page.locator('.thread-loading')).toHaveCount(0);
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await expect(page.locator('.prompt-input')).toBeVisible();
  });

  test('Given passive config and queue failure When sending message Then UI shows agent queue error, not generation error', async ({ page }) => {
    await installPassiveThreadAgentMock(page, { queueFails: true });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.locator('.prompt-input').fill('Message for external agent');

    await expect(page.getByRole('button', { name: 'QUEUE' })).toBeVisible();
    await page.locator('.prompt-input').press('Meta+Enter');

    await expect(page.locator('[data-testid="error-banner"]')).toContainText('Agent Queue Error');
    await expect(page.locator('[data-testid="error-banner"]')).not.toContainText('Generation Failed');
  });

  test('Given drawn annotations and unchecked workspace toggle When queueing message Then annotation capture stays one-shot and checkbox stays unchecked', async ({ page }) => {
    await installPassiveThreadAgentMock(page);

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN' }).click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const captureToggle = page.locator('.workspace-capture-toggle input');
    await expect(captureToggle).not.toBeChecked();

    await drawViewportAnnotation(page);
    await expect(page.locator('.workspace-capture-hint')).toContainText(
      'Enabled automatically because the current viewport has drawn annotations.',
    );
    await expect(captureToggle).not.toBeChecked();

    await page.locator('.prompt-input').fill('Message with annotation');
    await page
      .getByRole('button', { name: 'QUEUE' })
      .evaluate((element: HTMLButtonElement) => element.click());

    await expect.poll(async () =>
      page.evaluate(
        () =>
          (window as Window & typeof globalThis & {
            __MOCK_AGENT_DIALOGUE_CALLS__?: { queue: number; generate: number };
          }).__MOCK_AGENT_DIALOGUE_CALLS__,
      ),
    ).toMatchObject({ queue: 1, generate: 0 });

    await expect(page.locator('.workspace-capture-hint')).toHaveCount(0);
    await expect(captureToggle).not.toBeChecked();
  });

  test('Given queued inbox work and pending confirm When opening projects Then project card shows both badges', async ({ page }) => {
    await installPassiveThreadAgentMock(page, {
      queuedCount: 2,
      pendingConfirm: 'review-lens-fit',
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');

    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const card = page.locator('.project-card').filter({ hasText: 'Passive MCP Thread' }).first();
    await expect(card).toContainText('INBOX 2');
    await expect(card).toContainText('CONFIRM');
  });
});
