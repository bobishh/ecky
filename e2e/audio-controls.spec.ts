import { test, expect, type Page } from '@playwright/test';

type SpeechMockMessageMode = 'agentDraft' | 'llmReply' | 'error';

async function installSpeechPolicyMocks(page: Page, mode: SpeechMockMessageMode, voiceConnected = false) {
  await page.route(/\/mock\/.*\.stl(\?.*)?$/, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: `solid speech-policy
facet normal 0 0 1
  outer loop
    vertex 0 0 0
    vertex 1 0 0
    vertex 0 1 0
  endloop
endfacet
endsolid speech-policy`,
    });
  });

  await page.addInitScript(({ mockMode, voiceConnected }) => {
    const mockWindow = window as any;
    const nowSeconds = () => Math.floor(Date.now() / 1000);
    const bundle = {
      modelId: 'speech-policy-model',
      sourceKind: 'generated',
      contentHash: 'speech-policy-hash',
      fcstdPath: '/mock/speech-policy.FCStd',
      manifestPath: '/mock/speech-policy-manifest.json',
      modelStlPath: '/mock/speech-policy.stl',
      viewerAssets: [],
    };
    const isErrorMode = mockMode === 'error';
    const isAgentDraftMode = mockMode === 'agentDraft';
    const design = {
      title: 'Speech Policy Part',
      versionName: 'V-mcp-20260501',
      response: isAgentDraftMode
        ? 'Draft update via macro replacement.'
        : 'Final LLM reply.',
      interactionMode: 'design',
      macroCode: '(model)',
      macroDialect: 'ecky',
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'freecad',
      uiSpec: { fields: [] },
      initialParams: {},
      postProcessing: null,
    };
    const manifest = {
      modelId: 'speech-policy-model',
      sourceKind: 'generated',
      document: {
        documentName: 'Speech Policy Part',
        documentLabel: 'Speech Policy Part',
        objectCount: 1,
        warnings: [],
      },
      parts: [],
      parameterGroups: [],
      selectionTargets: [],
      warnings: [],
      enrichmentState: { status: 'none', proposals: [] },
    };
    const message = {
      id: isErrorMode ? 'msg-error' : 'msg-agent-draft',
      role: 'assistant',
      content: isErrorMode ? 'Render Error: FreeCAD failed with exit code 1.' : 'Primary updated V-mcp-20260501.',
      status: isErrorMode ? 'error' : 'success',
      output: isErrorMode ? null : design,
      usage: null,
      artifactBundle: isErrorMode ? null : bundle,
      modelManifest: isErrorMode ? null : manifest,
      agentOrigin: isAgentDraftMode ? {
        hostLabel: 'Codex',
        clientKind: 'mcp',
        agentLabel: 'Primary',
        llmModelId: 'gpt-5',
        llmModelLabel: 'GPT-5',
        sessionId: 'session-agent',
        createdAt: nowSeconds(),
      } : null,
      imageData: null,
      visualKind: null,
      attachmentImages: [],
      timestamp: nowSeconds(),
    };
    const thread = {
      id: 'thread-speech-policy',
      title: 'Speech Policy Thread',
      summary: '',
      updatedAt: nowSeconds(),
      versionCount: isErrorMode ? 0 : 1,
      pendingCount: 0,
      queuedCount: 0,
      errorCount: isErrorMode ? 1 : 0,
      status: 'active',
      finalizedAt: null,
      pendingConfirm: null,
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'freecad',
      messages: [message],
    };

    mockWindow.__SPOKEN_TEXT__ = [];
    mockWindow.__SPEECH_CANCEL_COUNT__ = 0;
    class MockSpeechSynthesisUtterance {
      text: string;
      rate = 1;
      pitch = 1;
      volume = 1;
      constructor(text: string) {
        this.text = text;
      }
    }
    Object.defineProperty(window, 'SpeechSynthesisUtterance', {
      value: MockSpeechSynthesisUtterance,
      configurable: true,
    });
    Object.defineProperty(window, 'speechSynthesis', {
      value: {
        cancel: () => { mockWindow.__SPEECH_CANCEL_COUNT__ += 1; },
        speak: (utterance: MockSpeechSynthesisUtterance) => {
          mockWindow.__SPOKEN_TEXT__.push(utterance.text);
        },
      },
      configurable: true,
    });

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    let nextHandler = 0;
    const handlers = new Map<string, number>();
    window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
      const id = ++nextHandler;
      mockWindow[`_${id}`] = callback;
      return id;
    };
    const voice = {
      eckyThreadId: 'another-voice-project', phase: 'connected', detail: null, terminalId: 'agy-voice:another-voice-project',
    };
    mockWindow.__END_AGY_VOICE__ = () => {
      const handler = handlers.get('agy-voice-updated');
      mockWindow[`_${handler}`]?.({ payload: { ...voice, phase: 'idle' } });
    };
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'plugin:event|listen') {
        const handler = Number(args?.handler);
        handlers.set(String(args?.event), handler);
        return handler;
      }
      if (cmd === 'get_agy_voice') return voiceConnected ? voice : null;
      if (cmd === 'get_agent_activity') return { events: [], latestCursor: 0 };
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
          microwave: { humId: null, dingId: null, muted: false },
          voice: { sttLanguageCode: 'en-US' },
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
          defaultEngineKind: 'ecky',
          defaultSourceLanguage: 'ecky',
          defaultGeometryBackend: 'freecad',
          maxGenerationAttempts: 1,
          maxVerifyAttempts: 0,
        };
      }
      if (cmd === 'save_config') return null;
      if (cmd === 'list_models') return ['gpt-4.1', 'gpt-4.1-mini'];
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          directOcct: { available: false, detail: 'Not configured', path: null },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: {
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'freecad',
          },
        };
      }
      if (cmd === 'plugin:fs|exists') return true;
      if (cmd === 'plugin:fs|size') return 1024;
      if (cmd === 'check_freecad') return true;
      if (cmd === 'get_default_macro') return '(model)';
      if (cmd === 'get_history') return [thread];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_thread_latest_version') return isErrorMode ? null : message;
      if (cmd === 'get_thread_messages_page') {
        return { messages: [message], nextBefore: null, hasMore: false };
      }
      if (cmd === 'get_thread') return thread;
      if (cmd === 'get_thread_window_layout') return null;
      if (cmd === 'save_thread_window_layout') return null;
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: args?.threadId ?? null,
          connectionState: 'none',
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
      if (cmd === 'save_last_design') return null;
      if (cmd === 'update_version_preview') return null;
      return null;
    };
  }, { mockMode: mode, voiceConnected });
}

test.describe('Audio controls', () => {
  test('Given idle workbench, audio mute toggle remains visible', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    await page.getByRole('button', { name: 'APP', exact: true }).click();

    await expect(page.locator('.microwave-unit')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'AUDIO ON' })).toBeVisible();
  });

  test('Given audio toggle, muting keeps control available for unmute', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    await page.getByRole('button', { name: 'APP', exact: true }).click();

    const toggle = page.getByRole('button', { name: 'AUDIO ON' });
    await expect(toggle).toBeVisible();
    await toggle.click();

    await expect(page.getByRole('button', { name: 'AUDIO OFF' })).toBeVisible();
  });

  test('Given the latest reply is an MCP draft update When the project opens Then speech stays silent', async ({ page }) => {
    await installSpeechPolicyMocks(page, 'agentDraft');

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN', exact: true }).click();

    await expect(page.getByRole('region', { name: 'Agent notifications' })).toContainText('Draft update via macro replacement');
    await page.waitForTimeout(750);

    await expect.poll(() => page.evaluate(() => (window as any).__SPOKEN_TEXT__)).toEqual([]);
  });

  test('Given the latest reply is an LLM reply When the project opens Then Ecky reads it', async ({ page }) => {
    await installSpeechPolicyMocks(page, 'llmReply');

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN', exact: true }).click();

    await expect(page.getByRole('region', { name: 'Agent notifications' })).toContainText('Final LLM reply.');
    await expect.poll(() => page.evaluate(() => (window as any).__SPOKEN_TEXT__)).toEqual([
      'Final LLM reply.',
    ]);
  });

  test('Given the latest reply is an error When the project opens Then Ecky reads the raw error', async ({ page }) => {
    await installSpeechPolicyMocks(page, 'error');

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN', exact: true }).click();

    await expect(page.getByRole('region', { name: 'Agent notifications' })).toContainText('Render Error: FreeCAD failed with exit code 1.');
    await expect.poll(() => page.evaluate(() => (window as any).__SPOKEN_TEXT__)).toEqual([
      'Render Error: FreeCAD failed with exit code 1.',
    ]);
  });

  test('Given voice is connected in another project When a model reply appears Then Ecky stays silent and keeps audio enabled', async ({ page }) => {
    await installSpeechPolicyMocks(page, 'llmReply', true);
    await page.goto('/');
    await expect.poll(() => page.evaluate(() => (window as any).__SPEECH_CANCEL_COUNT__)).toBeGreaterThan(0);
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'OPEN', exact: true }).click();
    await expect(page.getByRole('region', { name: 'Agent notifications' })).toContainText('Final LLM reply.');
    await expect(page.evaluate(() => (window as any).__SPOKEN_TEXT__)).resolves.toEqual([]);

    await page.evaluate(async () => {
      (window as any).__END_AGY_VOICE__();
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    });
    await expect(page.evaluate(() => (window as any).__SPOKEN_TEXT__)).resolves.toEqual([]);
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByRole('button', { name: 'APP', exact: true }).click();
    await expect(page.getByRole('button', { name: 'AUDIO ON' })).toBeVisible();
  });
});
