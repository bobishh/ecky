import { expect, test, type Page } from '@playwright/test';

type ProviderMockMode = 'happy' | 'startFailure' | 'turnFailure' | 'controls' | 'delayed';

const eckyThread = {
  id: 'ecky-thread-1', title: 'Gearbox housing', summary: 'Current target: gearbox housing', updatedAt: 1787263200,
  versionCount: 0, pendingCount: 0, queuedCount: 0, errorCount: 0, status: 'active',
  finalizedAt: null, pendingConfirm: null, engineKind: 'eckyRust',
  sourceLanguage: 'eckyScheme', geometryBackend: 'eckyRust', messages: [{
    id: 'ecky-version-1', role: 'assistant', content: 'Housing V1 generated.', status: 'success', timestamp: 1787262900,
    output: { interactionMode: 'direct', title: 'Housing V1', versionName: 'V1', response: 'Housing V1 generated.' },
  }],
};

const binding = {
  eckyThreadId: eckyThread.id, codexThreadId: 'codex-owned-by-ecky-7', label: eckyThread.title,
  cwd: '/workspace/gearbox', bootstrapVersion: 1, createdAt: 1787263200, updatedAt: 1787263200,
};

const initialSnapshot = {
  binding,
  messages: [
    { id: 'codex:user-1', role: 'user', content: 'Keep wall thickness at 3 mm.', status: 'success', timestamp: 1787263000 },
    { id: 'codex:assistant-1', role: 'assistant', content: 'Wall thickness locked. Ready for mounting ribs.', status: 'success', timestamp: 1787263010 },
  ],
  liveMessages: [],
  turnTraces: [],
  nextCursor: 'older-cursor-1', backwardsCursor: 'newer-cursor-1',
  runtime: { phase: 'idle', activeTurnId: null, error: null }, queue: [],
};

const agyBinding = {
  eckyThreadId: eckyThread.id, agyConversationId: 'agy-owned-by-ecky-8', label: eckyThread.title,
  cwd: '/workspace/gearbox', bootstrapVersion: 1, createdAt: 1787263200, updatedAt: 1787263200,
};

const initialAgySnapshot = {
  binding: agyBinding,
  messages: [
    { id: 'agy:user-1', role: 'user', content: 'Inspect current constraints.', status: 'success', timestamp: 1787263000 },
    { id: 'agy:assistant-1', role: 'assistant', content: 'Constraints inspected through Ecky MCP.', status: 'success', timestamp: 1787263010 },
  ],
  liveMessages: [], nextCursor: null, backwardsCursor: null,
  turnTraces: [],
  runtime: { phase: 'idle', activeTurnId: null, error: null }, queue: [],
  capabilities: { steer: false, stop: true },
};

async function installProviderMocks(page: Page, mode: ProviderMockMode, initiallyBound = false, persistedImage = false) {
  await page.addInitScript(({ mode, eckyThread, binding, initialSnapshot, agyBinding, initialAgySnapshot, initiallyBound, persistedImage }) => {
    const w = window as any;
    localStorage.clear();
    w.__CODEX_CALLS__ = [];
    w.__CODEX_BINDING__ = initiallyBound ? structuredClone(binding) : null;
    w.__CODEX_SNAPSHOT__ = structuredClone(initialSnapshot);
    if (persistedImage) {
      w.__CODEX_SNAPSHOT__.messages[0].attachments = [{
        path: '/workspace/gearbox/.ecky/attachments/gearbox-reference.png',
        name: 'gearbox-reference.png',
        explanation: 'Match this bearing shoulder.',
        dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
        kind: 'image',
      }];
    }
    w.__AGY_BINDING__ = initiallyBound ? structuredClone(agyBinding) : null;
    w.__AGY_SNAPSHOT__ = structuredClone(initialAgySnapshot);
    w.__EVENT_HANDLERS__ = {};
    w.__START_FAILURES__ = 0;
    w.__PROVIDER_WRITER_ACTIVATION_ERROR__ = null;
    w.__PROJECT_SOURCE_ERROR__ = null;
    w.__PENDING_CODEX_SENDS__ = [];
    w.__PENDING_QUEUE_REMOVALS__ = [];
    w.__RESOLVE_QUEUE_REMOVE__ = () => {
      const pending = w.__PENDING_QUEUE_REMOVALS__.shift();
      if (!pending) return;
      w.__CODEX_SNAPSHOT__.queue = w.__CODEX_SNAPSHOT__.queue.filter((entry: any) => entry.id !== pending.queueId);
      pending.resolve(structuredClone(w.__CODEX_SNAPSHOT__));
    };
    w.__RESOLVE_CODEX_SEND__ = () => {
      const pending = w.__PENDING_CODEX_SENDS__.shift();
      if (!pending) return;
      const sentAt = Math.floor(Date.now() / 1000);
      w.__CODEX_SNAPSHOT__ = {
        binding: structuredClone(binding),
        messages: [
          { id: 'codex:user-delayed', role: 'user', content: pending.prompt, status: 'pending', timestamp: sentAt },
          { id: 'codex:assistant-delayed', role: 'assistant', content: 'Dovetail checked.', status: 'success', timestamp: sentAt },
        ],
        liveMessages: [],
        turnTraces: [],
        nextCursor: null, backwardsCursor: 'newer-cursor-2', runtime: { phase: 'idle', activeTurnId: null, error: null }, queue: [],
      };
      w.__EMIT_CODEX_EVENT__('turn/completed');
    };
    w.__EMIT_CODEX_EVENT__ = (method: string, liveMessages?: unknown[], runtime?: unknown, turnTraces?: unknown[]) => w.__EVENT_HANDLERS__['codex-provider-updated']?.({
      event: 'codex-provider-updated', id: 1, payload: {
        threadId: binding.codexThreadId,
        method,
        ...(liveMessages ? { liveMessages } : {}),
        ...(runtime ? { runtime } : {}),
        ...(turnTraces ? { turnTraces } : {}),
      },
    });
    w.__EMIT_AGY_EVENT__ = (method: string, liveMessages?: unknown[], runtime?: unknown, turnTraces?: unknown[]) => w.__EVENT_HANDLERS__['agy-provider-updated']?.({
      event: 'agy-provider-updated', id: 1, payload: {
        conversationId: agyBinding.agyConversationId,
        method,
        ...(liveMessages ? { liveMessages } : {}),
        ...(runtime ? { runtime } : {}),
        ...(turnTraces ? { turnTraces } : {}),
      },
    });

    const config = {
      engines: [{ id: 'api-main', name: 'API Main', provider: 'openai', apiKey: 'sk-live', model: 'gpt-5', lightModel: 'gpt-5-mini', baseUrl: '', enabled: true }],
      selectedEngineId: 'api-main', freecadCmd: '', cadTextFontPath: '', projectsRoot: '', freecadLibraryRoots: [], assets: [],
      microwave: { humId: null, dingId: null, muted: true }, voice: { sttLanguageCode: 'en-US' },
      mcp: { port: null, maxSessions: null, mode: 'passive', primaryAgentId: null, promptTimeoutSecs: 1800, eckyAstAuthoring: false, autoAgents: [] },
      hasSeenOnboarding: true, connectionType: 'api_key', defaultEngineKind: 'eckyRust',
      providerModels: { codex: '', agy: '' },
      defaultSourceLanguage: 'eckyScheme', defaultGeometryBackend: 'eckyRust', maxGenerationAttempts: 1, maxVerifyAttempts: 0,
    };

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.metadata = {};
    window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
      const id = Math.floor(Math.random() * 1_000_000_000); w[`_${id}`] = callback; return id;
    };
    window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
      w.__CODEX_CALLS__.push({ cmd, args });
      if (cmd === 'plugin:event|listen') {
        const event = String(args?.event ?? ''); const handler = Number(args?.handler ?? -1);
        w.__EVENT_HANDLERS__[event] = w[`_${handler}`]; return 1;
      }
      if (cmd === 'plugin:event|unlisten') return null;
      if (cmd === 'get_config') return structuredClone(config);
      if (cmd === 'save_config') {
        if (w.__CONFIG_SAVE_ERROR__) throw 'config.edn: encode: invalid config field connection-type';
        return null;
      }
      if (cmd === 'list_agent_models') {
        const provider = String(args?.cmd ?? '');
        return {
          models: provider === 'agy'
            ? ['gemini-3.7-flash-high', 'claude-sonnet-4-6']
            : ['gpt-5.6', 'gpt-5.6-mini'],
          isLive: true,
        };
      }
      if (cmd === 'list_provider_models') {
        if (w.__PROVIDER_MODEL_ERROR__) throw w.__PROVIDER_MODEL_ERROR__;
        const provider = String(args?.provider ?? '');
        return {
          models: provider === 'agy'
            ? ['gemini-3.7-flash-high', 'claude-sonnet-4-6']
            : ['gpt-5.6', 'gpt-5.6-mini'],
          isLive: true,
        };
      }
      if (cmd === 'get_runtime_capabilities') return {
        freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
        build123d: { available: true, detail: 'Ready', path: '/mock/python3' }, mesh: { available: true, detail: 'Ready', path: null },
        recommendedAuthoringContext: { engineKind: 'eckyRust', sourceLanguage: 'eckyScheme', geometryBackend: 'eckyRust' },
      };
      if (cmd === 'check_freecad') return true;
      if (cmd === 'get_default_macro') return '(model)';
      if (cmd === 'get_history') return [structuredClone(eckyThread)];
      if (cmd === 'get_thread') return structuredClone(eckyThread);
      if (cmd === 'get_thread_messages_page') return { messages: structuredClone(eckyThread.messages), hasMore: false, nextBefore: null };
      if (cmd === 'get_project_source') {
        if (w.__PROJECT_SOURCE_ERROR__) throw w.__PROJECT_SOURCE_ERROR__;
        const lines = Array.from({ length: 120 }, (_, index) => index === 109
          ? '(param dryer_section_height 80mm)'
          : `; line ${index + 1}`);
        return {
          threadId: eckyThread.id,
          slug: 'c-dc939cfd',
          folder: '/Users/bogdan/Library/Application Support/com.alcoholics-audacious.ecky-cad/projects/c-dc939cfd',
          file: '/Users/bogdan/Library/Application Support/com.alcoholics-audacious.ecky-cad/projects/c-dc939cfd/model.ecky',
          source: lines.join('\n'),
        };
      }
      if (cmd === 'get_thread_latest_version' || cmd === 'get_last_design') return null;
      if (cmd === 'get_active_agent_sessions' || cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') return { threadId: args?.threadId ?? null, connectionState: 'disconnected', sessions: [], primaryAgentLabel: null, statusText: '', phase: null, busy: false, agentLabel: null, activityLabel: '', sessionId: null };
      if (cmd === 'get_mcp_server_status') return { running: true, endpointUrl: 'http://127.0.0.1:39249/mcp', sessions: [] };
      if (cmd === 'prepare_prompt_attachments') {
        return ((args?.attachments as any[]) ?? []).map((attachment: any) => ({
          ...attachment,
          path: `/workspace/gearbox/.ecky/attachments/${attachment.name}`,
          dataUrl: null,
        }));
      }
      if (cmd === 'activate_provider_writer') {
        if (w.__PROVIDER_WRITER_ACTIVATION_ERROR__) throw w.__PROVIDER_WRITER_ACTIVATION_ERROR__;
        w.__ACTIVE_PROVIDER_WRITER__ = structuredClone(args?.input ?? null);
        return null;
      }
      if (cmd === 'get_codex_takeover') return w.__CODEX_BINDING__ ? structuredClone(w.__CODEX_SNAPSHOT__) : null;
      if (cmd === 'get_agy_provider') return w.__AGY_BINDING__ ? structuredClone(w.__AGY_SNAPSHOT__) : null;
      if (cmd === 'get_agy_provider_messages') return { messages: [], nextCursor: null, backwardsCursor: null };
      if (cmd === 'get_codex_takeover_messages') return {
        messages: [{ id: 'codex:user-old', role: 'user', content: 'Original gearbox envelope: 120 × 80 mm.', status: 'success', timestamp: 1787250000 }],
        nextCursor: null, backwardsCursor: 'newer-cursor-2',
      };
      if (cmd === 'send_codex_takeover_prompt') {
        const prompt = String((args?.input as any)?.promptText ?? '');
        if (mode === 'delayed') {
          const queued = {
            id: 'queue-delayed', eckyThreadId: eckyThread.id, promptText: prompt,
            attachments: [], status: 'queued', error: null, createdAt: 1787263300, updatedAt: 1787263300,
          };
          w.__PENDING_CODEX_SENDS__.push({ prompt });
          w.__CODEX_SNAPSHOT__.queue = [queued];
          return structuredClone(w.__CODEX_SNAPSHOT__);
        }
        if (mode === 'startFailure' && w.__START_FAILURES__++ === 0) throw 'thread/start failed: Codex login expired (401 raw body)';
        w.__CODEX_BINDING__ = structuredClone(binding);
        if (mode === 'turnFailure') {
          w.__CODEX_SNAPSHOT__.queue = [{ id: 'queue-failed', eckyThreadId: eckyThread.id, promptText: prompt, attachments: [], status: 'failed', error: 'turn/start failed: workspace sandbox denied write', createdAt: 1787263300, updatedAt: 1787263300 }];
          return structuredClone(w.__CODEX_SNAPSHOT__);
        }
        if (mode === 'controls') {
          w.__CODEX_SNAPSHOT__.queue.push({ id: 'queue-2', eckyThreadId: eckyThread.id, promptText: prompt, attachments: [], status: 'queued', error: null, createdAt: 1787263300, updatedAt: 1787263300 });
          return structuredClone(w.__CODEX_SNAPSHOT__);
        }
        w.__CODEX_SNAPSHOT__ = {
          binding: structuredClone(binding),
          messages: [
            { id: 'codex:user-2', role: 'user', content: prompt, status: 'success', timestamp: 1787263300 },
            { id: 'codex:assistant-2', role: 'assistant', content: 'Four constrained ribs added and previewed.', status: 'success', timestamp: 1787263310 },
          ],
          liveMessages: [],
          turnTraces: [],
          nextCursor: null, backwardsCursor: 'newer-cursor-2', runtime: { phase: 'idle', activeTurnId: null, error: null }, queue: [],
        };
        return structuredClone(w.__CODEX_SNAPSHOT__);
      }
      if (cmd === 'send_agy_provider_prompt') {
        const prompt = String((args?.input as any)?.promptText ?? '');
        if (mode === 'startFailure') throw 'Antigravity CLI 1.0.6 does not support bidirectional stream-json; Ecky requires >=1.1.15. Run `agy update`.';
        w.__AGY_BINDING__ = structuredClone(agyBinding);
        w.__AGY_SNAPSHOT__ = {
          ...structuredClone(initialAgySnapshot),
          messages: [
            { id: 'agy:user-2', role: 'user', content: prompt, status: 'success', timestamp: 1787263300 },
            { id: 'agy:assistant-2', role: 'assistant', content: 'Current model inspected through Ecky MCP.', status: 'success', timestamp: 1787263310 },
          ],
        };
        return structuredClone(w.__AGY_SNAPSHOT__);
      }
      if (cmd === 'dispatch_agy_prompt_queue') return structuredClone(w.__AGY_SNAPSHOT__);
      if (cmd === 'stop_agy_provider') { w.__AGY_SNAPSHOT__.runtime.phase = 'stopping'; return structuredClone(w.__AGY_SNAPSHOT__); }
      if (cmd === 'retry_agy_queued_prompt' || cmd === 'remove_agy_queued_prompt') return structuredClone(w.__AGY_SNAPSHOT__);
      if (cmd === 'steer_codex_takeover') {
        const prompt = String((args?.input as any)?.promptText ?? '');
        w.__CODEX_SNAPSHOT__.messages.push({
          id: `codex:steer:${w.__CODEX_SNAPSHOT__.messages.length}`,
          role: 'user', content: prompt, status: 'success', timestamp: Math.floor(Date.now() / 1000),
        });
        return structuredClone(w.__CODEX_SNAPSHOT__);
      }
      if (cmd === 'stop_codex_takeover') { w.__CODEX_SNAPSHOT__.runtime.phase = 'stopping'; return structuredClone(w.__CODEX_SNAPSHOT__); }
      if (cmd === 'dispatch_codex_prompt_queue') return structuredClone(w.__CODEX_SNAPSHOT__);
      if (cmd === 'retry_codex_queued_prompt') {
        const item = w.__CODEX_SNAPSHOT__.queue.find((entry: any) => entry.id === String(args?.queueId ?? ''));
        if (item) { item.status = 'queued'; item.error = null; } return structuredClone(w.__CODEX_SNAPSHOT__);
      }
      if (cmd === 'remove_codex_queued_prompt') {
        if (w.__DELAY_QUEUE_REMOVE__) {
          return new Promise((resolve) => w.__PENDING_QUEUE_REMOVALS__.push({ queueId: String(args?.queueId ?? ''), resolve }));
        }
        w.__CODEX_SNAPSHOT__.queue = w.__CODEX_SNAPSHOT__.queue.filter((entry: any) => entry.id !== String(args?.queueId ?? ''));
        return structuredClone(w.__CODEX_SNAPSHOT__);
      }
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      return null;
    };
  }, { mode, eckyThread, binding, initialSnapshot, agyBinding, initialAgySnapshot, initiallyBound, persistedImage });
}

async function selectCodexProvider(page: Page) {
  await page.getByRole('button', { name: 'Settings' }).click();
  const settings = page.locator('[data-window-id="settings"]');
  await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
  await expect(settings.getByRole('button', { name: 'CODEX', exact: true })).toBeVisible();
  await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
}

async function openDialogue(page: Page) {
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.getByRole('button', { name: 'OPEN' }).first().click();
  await page.getByRole('button', { name: 'DIALOGUE' }).click();
  await expect(page.getByRole('region', { name: 'Prompt panel' })).toBeVisible();
}

async function bootProviderDialogue(page: Page) {
  await page.goto('/'); await selectCodexProvider(page); await openDialogue(page);
  await expect(page.getByRole('button', { name: 'TAKE OVER CODEX' })).toHaveCount(0);
}

test.describe('Codex provider integration', () => {
  test('Given bound Codex and Agy conversations When each Ecky thread opens Then both read Ecky history without provider writer activation', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await bootProviderDialogue(page);
    let calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    const codexActivation = calls.findIndex((call: any) => call.cmd === 'activate_provider_writer'
      && call.args?.input?.provider === 'codex');
    const codexRead = calls.findIndex((call: any) => call.cmd === 'get_codex_takeover');
    expect(codexActivation).toBe(-1);
    expect(codexRead).toBeGreaterThanOrEqual(0);
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Wall thickness locked. Ready for mounting ribs.' })).toBeVisible();

    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Constraints inspected through Ecky MCP.' })).toBeVisible();

    calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    const agyActivation = calls.findIndex((call: any) => call.cmd === 'activate_provider_writer'
      && call.args?.input?.provider === 'agy');
    const agyRead = calls.findIndex((call: any) => call.cmd === 'get_agy_provider');
    expect(agyActivation).toBe(-1);
    expect(agyRead).toBeGreaterThanOrEqual(0);
  });

  test('Given Codex has another active writer When Ecky thread opens Then durable Ecky history renders without owner error', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__PROVIDER_WRITER_ACTIVATION_ERROR__ = 'thread codex-owned-by-ecky-7 already has an active writer';
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    await expect(page.locator('.trail-assistant').filter({ hasText: 'Wall thickness locked. Ready for mounting ribs.' })).toBeVisible();
    await expect(page.locator('.provider-conversation-error')).toHaveCount(0);
    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls.some((call: any) => call.cmd === 'activate_provider_writer'
      && call.args?.input?.provider === 'codex')).toBe(false);
    expect(calls.some((call: any) => call.cmd === 'get_codex_takeover')).toBe(true);
  });

  test('Given Provider settings When AGY is selected Then Dialogue routes the Ecky thread to the Agy adapter', async ({ page }) => {
    await installProviderMocks(page, 'happy');
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    const modelField = settings.locator('.provider-model-field');
    await modelField.locator('.select-trigger').click();
    await modelField.getByRole('button', { name: 'claude-sonnet-4-6', exact: true }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await openDialogue(page);

    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Inspect the current model through Ecky MCP.');
    await page.getByRole('button', { name: 'SEND TO AGY' }).click();

    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls.find((call: any) => call.cmd === 'save_config')?.args?.config?.connectionType).toBe('provider:agy');
    expect(calls.find((call: any) => call.cmd === 'save_config')?.args?.config?.providerModels).toEqual({
      codex: '',
      agy: 'claude-sonnet-4-6',
    });
    expect(calls).toContainEqual({
      cmd: 'send_agy_provider_prompt',
      args: { input: { eckyThreadId: 'ecky-thread-1', promptText: 'Inspect the current model through Ecky MCP.', attachments: [] } },
    });
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Current model inspected through Ecky MCP.' })).toBeVisible();
  });

  test('Given Codex and AGY return LaTeX When Dialogue renders each answer Then complete formulas use math layout and an incomplete formula stays readable', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      const content = [
        'Углы от $50^\\circ$ до $75^\\circ$.',
        'Сторона $30\\text{ см}$, диаметр $\\approx 2.4\\text{ м}$.',
        'Незакрытая $формула',
      ].join('\n');
      (window as any).__CODEX_SNAPSHOT__.messages[1].content = content;
      (window as any).__AGY_SNAPSHOT__.messages[1].content = content;
    });

    await selectCodexProvider(page);
    await openDialogue(page);
    let answer = page.locator('.trail-assistant').filter({ hasText: 'Углы от' });
    await expect(answer.locator('.provider-math')).toHaveCount(4);
    await expect(answer.locator('.provider-math .katex')).toHaveCount(4);
    await expect(answer).toContainText('Незакрытая $формула');

    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();

    answer = page.locator('.trail-assistant').filter({ hasText: 'Углы от' });
    await expect(answer.locator('.provider-math')).toHaveCount(4);
    await expect(answer.locator('.provider-math .katex')).toHaveCount(4);
    await expect(answer).toContainText('Незакрытая $формула');
  });

  test('Given Codex and AGY return Markdown with Mermaid When Dialogue renders each answer Then rich content and diagrams appear and invalid Mermaid exposes its error', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      const content = [
        '## Геометрия',
        '',
        '**Частота:** `3V`',
        '',
        '- PLAN',
        '- BUILD',
        '',
        'Угол $60^\\circ$.',
        '',
        '```mermaid',
        'flowchart LR',
        '  PLAN --> BUILD',
        '```',
        '',
        '```mermaid',
        'flowchart LR',
        '  BROKEN -->',
        '```',
      ].join('\n');
      (window as any).__CODEX_SNAPSHOT__.messages[1].content = content;
      (window as any).__AGY_SNAPSHOT__.messages[1].content = content;
    });

    const assertRichAnswer = async () => {
      const answer = page.locator('.trail-assistant').filter({ hasText: 'Геометрия' });
      await expect(answer.getByRole('heading', { name: 'Геометрия', level: 2 })).toBeVisible();
      await expect(answer.locator('strong')).toContainText('Частота:');
      await expect(answer.locator('code')).toContainText('3V');
      await expect(answer.getByRole('listitem')).toHaveCount(2);
      await expect(answer.locator('.provider-math .katex')).toHaveCount(1);
      await expect(answer.locator('.provider-mermaid svg')).toHaveCount(1, { timeout: 10_000 });
      await expect(answer.locator('.provider-mermaid-error')).toContainText(/Parse error|Syntax error/i);
    };

    await selectCodexProvider(page);
    await openDialogue(page);
    await assertRichAnswer();

    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await assertRichAnswer();
  });

  test('Given provider history contains an image When dialogue reloads Then the original attachment remains visible', async ({ page }) => {
    await installProviderMocks(page, 'happy', true, true);
    await bootProviderDialogue(page);

    const image = page.locator('.trail-user .trail-image');
    await expect(image).toHaveCount(1);
    await expect(image).toHaveAttribute('src', 'data:image/png;base64,iVBORw0KGgo=');
  });

  test('Given Codex persisted a generated concept sketch When dialogue reloads Then the assistant image remains visible', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.messages[1].attachments = [{
        path: '/workspace/gearbox/.ecky/attachments/engine-sketch.png',
        name: 'engine-sketch.png',
        explanation: 'Generated concept sketch.',
        dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
        kind: 'image',
      }];
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    const image = page.locator('.trail-assistant .trail-image');
    await expect(image).toHaveCount(1);
    await expect(image).toHaveAttribute('src', 'data:image/png;base64,iVBORw0KGgo=');
  });

  test('Given Codex persisted an Ecky sketch MCP result When dialogue reloads Then the sketch result remains visible', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.messages.push({
        id: 'codex:mcp:engine-layout',
        role: 'assistant',
        content: 'Ecky sketch draft created · engine-layout · 1 sketch.',
        status: 'success',
        timestamp: 1787263020,
      });
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    await expect(page.locator('.trail-assistant').filter({ hasText: 'Ecky sketch draft created · engine-layout · 1 sketch.' })).toBeVisible();
  });

  test('Given a Codex provider image attachment When sending Then the provider receives it and the composer clears it', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await bootProviderDialogue(page);

    await page.evaluate(() => {
      const container = document.querySelector('.prompt-container');
      if (!container) throw new Error('Prompt container missing');
      const transfer = new DataTransfer();
      transfer.items.add(new File(['image'], 'gearbox-reference.png', { type: 'image/png' }));
      container.dispatchEvent(new DragEvent('drop', {
        bubbles: true,
        cancelable: true,
        dataTransfer: transfer,
      }));
    });

    await expect(page.locator('.attachment-item')).toContainText('gearbox-reference.png');
    await page.locator('.att-explanation').fill('Match this bearing shoulder.');
    await page.locator('.prompt-input').fill('Update the housing from this reference.');

    const send = page.getByRole('button', { name: 'SEND TO CODEX' });
    await expect(send).toBeEnabled();
    await send.click();

    await expect(page.locator('.attachment-item')).toHaveCount(0);
    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    const sendCall = calls.find((call: any) => call.cmd === 'send_codex_takeover_prompt');
    expect(sendCall).toEqual({
      cmd: 'send_codex_takeover_prompt',
      args: {
        input: {
          eckyThreadId: 'ecky-thread-1',
          promptText: 'Update the housing from this reference.',
          attachments: [{
            path: '/workspace/gearbox/.ecky/attachments/gearbox-reference.png',
            name: 'gearbox-reference.png',
            explanation: 'Match this bearing shoulder.',
            dataUrl: null,
            kind: 'image',
          }],
        },
      },
    });
  });

  test('Given provider delivery fails When sending an image Then raw error shows and the complete draft returns', async ({ page }) => {
    await installProviderMocks(page, 'startFailure', true);
    await bootProviderDialogue(page);

    await page.evaluate(() => {
      const container = document.querySelector('.prompt-container');
      if (!container) throw new Error('Prompt container missing');
      const transfer = new DataTransfer();
      transfer.items.add(new File(['image'], 'failed-reference.png', { type: 'image/png' }));
      container.dispatchEvent(new DragEvent('drop', {
        bubbles: true,
        cancelable: true,
        dataTransfer: transfer,
      }));
    });

    await expect(page.locator('.attachment-item')).toContainText('failed-reference.png');
    await page.locator('.att-explanation').fill('Keep the failed reference note.');
    await page.locator('.prompt-input').fill('Try this reference.');
    await page.getByRole('button', { name: 'SEND TO CODEX' }).click();

    await expect(page.locator('.provider-conversation-error')).toContainText('Codex login expired (401 raw body)');
    await expect(page.locator('.prompt-input')).toHaveValue('Try this reference.');
    await expect(page.locator('.attachment-item')).toContainText('failed-reference.png');
    await expect(page.locator('.att-explanation')).toHaveValue('Keep the failed reference note.');
  });

  test('Given an Ecky thread switched from AGY to Codex When config saves once Then the next message uses Codex runtime', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await openDialogue(page);
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Constraints inspected through Ecky MCP.' })).toBeVisible();

    await page.getByRole('button', { name: 'Settings' }).click();
    await settings.getByRole('button', { name: 'CODEX', exact: true }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Wall thickness locked. Ready for mounting ribs.' })).toBeVisible();
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Constraints inspected through Ecky MCP.' })).toHaveCount(0);

    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Back on Codex.');
    await page.getByRole('button', { name: 'SEND TO CODEX' }).click();
    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls.filter((call: any) => call.cmd === 'send_codex_takeover_prompt')).toHaveLength(1);
    expect(calls.filter((call: any) => call.cmd === 'send_agy_provider_prompt')).toHaveLength(0);
  });

  test('Given Codex provider settings When models are fetched Then real provider models render in the shared dropdown without API fallbacks', async ({ page }) => {
    await installProviderMocks(page, 'happy');
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();

    await settings.getByRole('button', { name: 'FETCH MODELS' }).click();
    const modelField = settings.locator('.provider-model-field');
    await modelField.locator('.select-trigger').click();
    await expect(modelField.getByRole('button', { name: 'gpt-5.6', exact: true })).toBeVisible();
    await expect(modelField.getByRole('button', { name: 'gpt-4o', exact: true })).toHaveCount(0);
    await modelField.getByRole('button', { name: 'gpt-5.6', exact: true }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();

    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls.some((call: any) => call.cmd === 'list_provider_models' && call.args?.provider === 'codex')).toBe(true);
    expect(calls.findLast((call: any) => call.cmd === 'save_config')?.args?.config?.providerModels?.codex).toBe('gpt-5.6');
  });

  test('Given provider model discovery fails When models are fetched Then raw provider error remains visible', async ({ page }) => {
    await installProviderMocks(page, 'happy');
    await page.goto('/');
    await page.evaluate(() => { (window as any).__PROVIDER_MODEL_ERROR__ = 'model/list failed: subscription catalog unavailable (raw)'; });
    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'FETCH MODELS' }).click();
    await expect(settings.locator('.status-msg')).toContainText('model/list failed: subscription catalog unavailable (raw)');
  });

  test('Given an old Agy CLI When first delivery starts Then raw version failure remains retryable', async ({ page }) => {
    await installProviderMocks(page, 'startFailure');
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await openDialogue(page);
    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Inspect the current target.');
    await page.getByRole('button', { name: 'SEND TO AGY' }).click();
    await expect(page.locator('.provider-conversation-error')).toContainText('requires >=1.1.15');
    await expect(input).toHaveValue('Inspect the current target.');
  });

  test('Given active Agy work When Dialogue opens Then queue and STOP render without fake STEER', async ({ page }) => {
    await installProviderMocks(page, 'controls', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__AGY_SNAPSHOT__.runtime = { phase: 'active', activeTurnId: 'agy-turn-1', error: null };
      (window as any).__AGY_SNAPSHOT__.liveMessages = [
        { id: 'agy:work-1', role: 'assistant', content: 'WORKING · Inspecting authored constraints', status: 'working', timestamp: 2, providerEventKind: 'activity' },
        { id: 'agy:work-2', role: 'assistant', content: 'WORKING · Validating current model', status: 'working', timestamp: 3, providerEventKind: 'activity' },
        { id: 'agy:answer-1', role: 'assistant', content: 'Проверяю соединение перед изменением.', status: 'working', timestamp: 4, providerEventKind: 'assistant' },
      ];
      (window as any).__AGY_SNAPSHOT__.messages.push({
        id: 'agy:user:active', role: 'user', content: 'Inspect the current joint.', status: 'success', timestamp: 1,
      });
      (window as any).__AGY_SNAPSHOT__.queue = [
        {
          id: 'agy-queue-active', eckyThreadId: 'ecky-thread-1', promptText: 'Inspect the current joint.',
          status: 'sending', error: null, createdAt: 1, updatedAt: 1,
        },
        {
          id: 'agy-queue-next', eckyThreadId: 'ecky-thread-1', promptText: 'Add the next rib.',
          status: 'queued', error: null, createdAt: 2, updatedAt: 2,
        },
      ];
    });
    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'AGY' }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await openDialogue(page);
    await expect(page.getByRole('region', { name: 'Agy prompt queue' })).toContainText('Add the next rib.');
    await expect(page.getByRole('region', { name: 'Agy prompt queue' })).not.toContainText('Inspect the current joint.');
    await expect(page.getByRole('region', { name: 'Agy prompt queue' })).not.toContainText('SENDING');
    await expect(page.getByRole('button', { name: 'STOP' })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'STEER' })).toHaveCount(0);
    const agyActivity = page.getByRole('region', { name: 'Agy working activity' });
    await expect(agyActivity).toHaveCount(1);
    await expect(agyActivity.getByText('2 EVENTS')).toBeVisible();
    await expect(agyActivity.locator('.provider-working__summary')).toHaveText('WORKING · Validating current model');
    await expect(agyActivity).not.toContainText('SYSTEM MESSAGE');
    await expect(agyActivity).not.toContainText('UNKNOWN');
    await expect(agyActivity).not.toContainText('Проверяю соединение перед изменением.');
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Проверяю соединение перед изменением.' })).toBeVisible();
    await expect(agyActivity.getByText('Inspecting authored constraints')).not.toBeVisible();
    await agyActivity.getByLabel('Show Agy working details').click();
    await expect(agyActivity.getByRole('listitem')).toHaveCount(2);
    await expect(agyActivity.getByText('Inspecting authored constraints')).toBeVisible();
    await page.getByPlaceholder(/Type a question or design change/i).fill('Then inspect the fillets.');
    await expect(page.getByRole('button', { name: 'QUEUE' })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'STOP' })).toHaveClass(/provider-control--stop/);
  });

  test('Given Codex provider mode When first message is sent Then Ecky creates its owned Codex conversation automatically', async ({ page }) => {
    await installProviderMocks(page, 'happy'); await bootProviderDialogue(page);
    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Add four constrained mounting ribs.');
    await page.getByRole('button', { name: 'SEND TO CODEX' }).click();
    await expect(page.locator('.trail-assistant').last()).toContainText('Four constrained ribs added and previewed.');
    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls).toContainEqual({ cmd: 'send_codex_takeover_prompt', args: { input: { eckyThreadId: 'ecky-thread-1', promptText: 'Add four constrained mounting ribs.', attachments: [] } } });
    expect(calls.some((call: any) => call.cmd === 'list_codex_threads' || call.cmd === 'take_over_codex_thread')).toBe(false);
  });

  test('Given slow Codex delivery When user sends Then pending input lives only in queue until Codex accepts it', async ({ page }) => {
    await installProviderMocks(page, 'delayed', true); await bootProviderDialogue(page);
    await expect(page.getByText('Housing V1 generated.')).toBeVisible();
    await expect(page.locator('.trail-version-event').filter({ hasText: 'Housing V1 generated.' })).toBeVisible();
    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Dovetail не вижу');
    await page.getByRole('button', { name: 'SEND TO CODEX' }).click();

    const queued = page.getByRole('region', { name: 'Codex prompt queue' });
    await expect(queued).toContainText('Dovetail не вижу');
    await expect(page.locator('.trail-user').filter({ hasText: 'Dovetail не вижу' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'SENDING...' })).toHaveCount(0);
    await input.fill('Next queued prompt');
    await expect(page.getByRole('button', { name: 'SEND TO CODEX' })).toBeEnabled();
    await input.fill('');

    await page.evaluate(() => (window as any).__RESOLVE_CODEX_SEND__());
    await expect(page.getByRole('region', { name: 'Codex prompt queue' })).toHaveCount(0);
    await expect(page.locator('.trail-user').filter({ hasText: 'Dovetail не вижу' })).toHaveCount(1);
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Dovetail checked.' })).toBeVisible();
    const visibleDialogue = await page.locator('.trail-item').evaluateAll((items) =>
      items.map((item) => item.textContent ?? '').filter((text) => text.includes('Dovetail')),
    );
    expect(visibleDialogue).toHaveLength(2);
    expect(visibleDialogue[0]).toContain('Dovetail не вижу');
    expect(visibleDialogue[1]).toContain('Dovetail checked.');
    await expect(page.getByText('Housing V1 generated.')).toBeVisible();

    await page.getByRole('button', { name: 'VERSIONS' }).click();
    await expect(page.getByText('Housing V1 generated.')).toBeVisible();
    await expect(page.getByText('Dovetail checked.')).toHaveCount(0);
    await page.getByRole('searchbox', { name: 'Search timeline' }).fill('housing v1');
    await expect(page.getByText('Housing V1 generated.')).toBeVisible();
  });

  test('Given Codex thread creation fails When first message is sent Then raw error remains and retry succeeds', async ({ page }) => {
    await installProviderMocks(page, 'startFailure'); await bootProviderDialogue(page);
    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Cut the bearing seat.'); await page.getByRole('button', { name: 'SEND TO CODEX' }).click();
    await expect(page.locator('.provider-conversation-error')).toContainText('thread/start failed: Codex login expired (401 raw body)');
    await expect(input).toHaveValue('Cut the bearing seat.');
    await page.getByRole('button', { name: 'SEND TO CODEX' }).click();
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Four constrained ribs added and previewed.' })).toBeVisible();
  });

  test('Given owned conversation When dialogue opens Then cursor history resumes without discovery UI', async ({ page }) => {
    await installProviderMocks(page, 'happy', true); await bootProviderDialogue(page);
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Wall thickness locked.' })).toBeVisible();
    await page.getByRole('button', { name: 'SHOW OLDER MESSAGES' }).click();
    await expect(page.locator('.trail-user').first()).toContainText('Original gearbox envelope');
    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls.filter((call: any) => call.cmd === 'get_codex_takeover_messages')).toEqual([{ cmd: 'get_codex_takeover_messages', args: { input: { eckyThreadId: 'ecky-thread-1', cursor: 'older-cursor-1', direction: 'older' } } }]);
  });

  test('Given local provider history initially fits one page When background backfill persists older turns Then older-page control appears', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => { (window as any).__CODEX_SNAPSHOT__.nextCursor = null; });
    await selectCodexProvider(page);
    await openDialogue(page);
    await expect(page.getByRole('button', { name: 'SHOW OLDER MESSAGES' })).toHaveCount(0);

    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.nextCursor = 'older-cursor-1';
      (window as any).__EMIT_CODEX_EVENT__('history/persisted');
    });

    await expect(page.getByRole('button', { name: 'SHOW OLDER MESSAGES' })).toBeVisible();
  });

  test('Given provider answer references current model source When user opens that reference Then Code shows the exact line and debug ids stay hidden', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.messages[1].content = [
        'Высота секции: 18 → 80 мм.',
        '',
        'Параметр: [model.ecky](/Users/bogdan/Library/Application%20Support/com.alcoholics-audacious.ecky-cad/projects/c-dc939cfd/model.ecky:110)',
        '',
        '`messageId: dc1fd5aa-a024-4d4e-97fe-a261845806d9`\\',
        '`modelId: generated-direct-occt-58b8b28e2cc2`',
      ].join('\n');
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    const answer = page.locator('.trail-assistant').filter({ hasText: 'Высота секции' });
    await expect(answer).toBeVisible();
    await expect(answer).not.toContainText('messageId');
    await expect(answer).not.toContainText('modelId');
    await answer.getByRole('button', { name: 'Open model.ecky at line 110' }).click();

    const codeWindow = page.locator('[data-window-id="code"]');
    await expect(codeWindow).toBeVisible();
    await expect(codeWindow.locator('.cm-activeLine')).toContainText('(param dryer_section_height 80mm)');
  });

  test('Given referenced model source cannot load When user opens the reference Then raw source error stays with the answer and Code remains closed', async ({ page }) => {
    await installProviderMocks(page, 'happy', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.messages[1].content =
        'Параметр: [model.ecky](/Users/bogdan/Library/Application%20Support/com.alcoholics-audacious.ecky-cad/projects/c-dc939cfd/model.ecky:110)';
      (window as any).__PROJECT_SOURCE_ERROR__ = 'project source missing (raw backend body)';
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    const answer = page.locator('.trail-assistant').filter({ hasText: 'Параметр' });
    await answer.getByRole('button', { name: 'Open model.ecky at line 110' }).click();

    await expect(answer.getByRole('alert')).toContainText('project source missing (raw backend body)');
    await expect(page.locator('[data-window-id="code"]')).not.toBeVisible();
  });

  test('Given an active Codex turn When stream deltas arrive Then live work renders without reloading transcript pages', async ({ page }) => {
    await installProviderMocks(page, 'controls', true); await bootProviderDialogue(page);
    const readsBefore = await page.evaluate(() =>
      (window as any).__CODEX_CALLS__.filter((call: any) => call.cmd === 'get_codex_takeover').length,
    );
    await page.evaluate(() => {
      const runtime = { phase: 'active', activeTurnId: 'turn-live-10', error: null };
      const thinking = {
        id: 'codex:codex-owned-by-ecky-7:reasoning-10', role: 'assistant',
        content: 'THINKING · Проверяю доступную глубину резьбы.', status: 'working', timestamp: 1787263400,
        providerEventKind: 'activity',
      };
      const tool = {
        id: 'codex:codex-owned-by-ecky-7:tool-10', role: 'assistant',
        content: 'USING TOOL · ecky_provider_mcp/ecky_ast_inspect', status: 'working', timestamp: 1787263401,
        providerEventKind: 'activity',
      };
      const answer = {
        id: 'codex:codex-owned-by-ecky-7:answer-10', role: 'assistant',
        content: 'Сейчас сверяю радиус и глубину посадки.', status: 'working', timestamp: 1787263402,
        providerEventKind: 'assistant',
      };
      (window as any).__CODEX_TRACE_MESSAGES__ = [thinking, tool, answer];
      (window as any).__CODEX_SNAPSHOT__.liveMessages = [thinking];
      (window as any).__EMIT_CODEX_EVENT__('item/reasoning/summaryTextDelta', [thinking], runtime);
    });

    const activity = page.getByRole('region', { name: 'Codex working activity' });
    await expect(activity).toHaveCount(1);
    await expect(activity.locator('.provider-working__summary')).toHaveText('THINKING · Проверяю доступную глубину резьбы.');
    await activity.getByLabel('Show Codex working details').click();
    await expect(activity.getByRole('listitem').filter({ hasText: 'Проверяю доступную глубину резьбы.' })).toBeVisible();
    await page.evaluate(() => {
      const messages = (window as any).__CODEX_TRACE_MESSAGES__;
      const runtime = { phase: 'active', activeTurnId: 'turn-live-10', error: null };
      (window as any).__CODEX_SNAPSHOT__.liveMessages = messages.slice(0, 2);
      (window as any).__EMIT_CODEX_EVENT__('item/started', messages.slice(0, 2), runtime);
    });
    await expect(activity.locator('.provider-working__summary')).toContainText('ecky_ast_inspect');
    await expect(activity.getByRole('listitem').filter({ hasText: 'Проверяю доступную глубину резьбы.' })).toBeVisible();
    await page.evaluate(() => {
      const messages = (window as any).__CODEX_TRACE_MESSAGES__;
      const runtime = { phase: 'active', activeTurnId: 'turn-live-10', error: null };
      (window as any).__CODEX_SNAPSHOT__.liveMessages = messages;
      (window as any).__EMIT_CODEX_EVENT__('item/agentMessage/delta', messages, runtime);
    });
    await expect(activity.getByText('2 EVENTS')).toBeVisible();
    await expect(activity.locator('.provider-working__summary')).toContainText('ecky_ast_inspect');
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Сейчас сверяю радиус и глубину посадки.' })).toBeVisible();
    await expect(activity.getByRole('listitem').filter({ hasText: 'Проверяю доступную глубину резьбы.' })).toBeVisible();
    await page.evaluate(() => {
      const snapshot = (window as any).__CODEX_SNAPSHOT__;
      const trace = {
        turnId: 'turn-live-10', status: 'interrupted', completedAt: 1787263403,
        messages: snapshot.liveMessages.map((message: any) => ({ ...message, status: 'discarded' })),
      };
      snapshot.liveMessages = [];
      snapshot.turnTraces = [trace];
      snapshot.runtime = { phase: 'idle', activeTurnId: null, error: null };
      (window as any).__EMIT_CODEX_EVENT__('turn/completed', [], snapshot.runtime, snapshot.turnTraces);
    });
    await expect(activity.getByText('STOPPED')).toBeVisible();
    await expect(activity.getByRole('listitem').filter({ hasText: 'Проверяю доступную глубину резьбы.' })).toBeVisible();
    await expect(activity.locator('.provider-working__summary')).toContainText('ecky_ast_inspect');
    await expect(activity.getByRole('listitem').allTextContents()).resolves.toEqual([
      'THINKING · Проверяю доступную глубину резьбы.',
      'USING TOOL · ecky_provider_mcp/ecky_ast_inspect',
    ]);
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Сейчас сверяю радиус и глубину посадки.' })).toBeVisible();
    await page.waitForTimeout(350);
    expect(await page.evaluate(() =>
      (window as any).__CODEX_CALLS__.filter((call: any) => call.cmd === 'get_codex_takeover').length,
    )).toBe(readsBefore);
  });

  test('Given a provider turn starts before activity arrives When thinking or failure follows Then Ecky confirms receipt and yields to exact provider state', async ({ page }) => {
    await installProviderMocks(page, 'controls', true); await bootProviderDialogue(page);
    await page.evaluate(() => {
      const snapshot = (window as any).__CODEX_SNAPSHOT__;
      snapshot.liveMessages = [];
      snapshot.runtime = { phase: 'active', activeTurnId: 'turn-starting-12', error: null };
      (window as any).__EMIT_CODEX_EVENT__('turn/started', [], snapshot.runtime);
    });

    const activity = page.getByRole('region', { name: 'Codex working activity' });
    await expect(activity.locator('.provider-working__summary')).toHaveText(
      'THINKING · Message received. Starting work.',
    );

    await page.evaluate(() => {
      const snapshot = (window as any).__CODEX_SNAPSHOT__;
      const thinking = {
        id: 'codex:codex-owned-by-ecky-7:reasoning-12',
        role: 'assistant',
        content: 'THINKING · Inspecting current constraints.',
        status: 'working',
        timestamp: 1787263600,
        providerEventKind: 'activity',
      };
      snapshot.liveMessages = [thinking];
      (window as any).__EMIT_CODEX_EVENT__(
        'item/reasoning/summaryTextDelta',
        snapshot.liveMessages,
        snapshot.runtime,
      );
    });
    await expect(activity.locator('.provider-working__summary')).toHaveText(
      'THINKING · Inspecting current constraints.',
    );

    await page.evaluate(() => {
      const snapshot = (window as any).__CODEX_SNAPSHOT__;
      snapshot.liveMessages = [];
      snapshot.runtime = {
        phase: 'error',
        activeTurnId: null,
        error: 'provider rejected turn: raw upstream body',
      };
      (window as any).__EMIT_CODEX_EVENT__(
        'thread/status/changed',
        snapshot.liveMessages,
        snapshot.runtime,
      );
    });
    await expect(activity).toHaveCount(0);
    await expect(page.locator('.provider-conversation-error')).toContainText(
      'provider rejected turn: raw upstream body',
    );
  });

  test('Given a successful provider turn When final answer arrives Then trace collapses before the answer', async ({ page }) => {
    await installProviderMocks(page, 'controls', true); await bootProviderDialogue(page);
    await page.evaluate(() => {
      const snapshot = (window as any).__CODEX_SNAPSHOT__;
      snapshot.messages = [
        ...snapshot.messages,
        { id: 'codex:final-11', role: 'assistant', content: 'Готово: посадка проверена.', status: 'success', timestamp: 1787263501 },
      ];
      snapshot.liveMessages = [];
      snapshot.turnTraces = [{
        turnId: 'turn-live-11', status: 'success', completedAt: 1787263500,
        messages: [
          { id: 'trace-1', role: 'assistant', content: 'THINKING · Проверяю посадку.', status: 'success', timestamp: 1787263498 },
          { id: 'trace-2', role: 'assistant', content: 'USING TOOL · ecky_ast_inspect', status: 'success', timestamp: 1787263499 },
        ],
      }];
      snapshot.runtime = { phase: 'idle', activeTurnId: null, error: null };
      (window as any).__EMIT_CODEX_EVENT__('turn/completed', [], snapshot.runtime, snapshot.turnTraces);
    });

    const activity = page.getByRole('region', { name: 'Codex working activity' });
    await expect(activity.getByText('WORKED')).toBeVisible();
    await expect(activity.getByText('Проверяю посадку.')).not.toBeVisible();
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Готово: посадка проверена.' })).toBeVisible();
    const timeline = await page.locator('.trail-list > .trail-item').allTextContents();
    expect(timeline.findIndex((item) => item.includes('WORKED')))
      .toBeLessThan(timeline.findIndex((item) => item.includes('Готово: посадка проверена.')));
  });

  test('Given active work When user steers, stops, and submits Then FIFO survives compaction', async ({ page }) => {
    await installProviderMocks(page, 'controls', true); await page.goto('/'); await selectCodexProvider(page);
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.runtime = { phase: 'active', activeTurnId: 'turn-live-9', error: null };
      (window as any).__CODEX_SNAPSHOT__.queue = [{ id: 'queue-1', eckyThreadId: 'ecky-thread-1', promptText: 'Polish bearing bore.', status: 'queued', error: null, createdAt: 1, updatedAt: 1 }];
    });
    await openDialogue(page); await page.evaluate(() => (window as any).__EMIT_CODEX_EVENT__('thread/compacted')); await page.waitForTimeout(350);
    await expect(page.getByRole('button', { name: 'STOP' })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'STOP' })).toHaveClass(/provider-control--stop/);
    await expect(page.getByRole('button', { name: 'STEER' })).toHaveClass(/provider-control--steer/);
    expect(await page.evaluate(() => (window as any).__CODEX_CALLS__.filter((call: any) => call.cmd === 'dispatch_codex_prompt_queue').length)).toBe(0);
    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Keep ribs symmetric.'); await page.getByRole('button', { name: 'STEER' }).click(); await page.getByRole('button', { name: 'STOP' }).click();
    await input.fill('Then add inspection fillets.'); await page.getByRole('button', { name: 'QUEUE' }).click();
    await expect(page.getByRole('region', { name: 'Codex prompt queue' })).toContainText('Then add inspection fillets.');
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.runtime = { phase: 'idle', activeTurnId: null, error: null };
      (window as any).__EMIT_CODEX_EVENT__('thread/status/changed');
    });
    await expect.poll(async () => page.evaluate(() =>
      (window as any).__CODEX_CALLS__.filter((call: any) => call.cmd === 'dispatch_codex_prompt_queue').length,
    )).toBe(1);
  });

  test('Given an active Codex turn When Cmd+Enter is pressed Then input steers visibly instead of entering FIFO', async ({ page }) => {
    await installProviderMocks(page, 'controls', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.runtime = { phase: 'active', activeTurnId: 'turn-steer', error: null };
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    const input = page.getByPlaceholder(/Type a question or design change/i);
    await expect(input).toHaveAttribute('placeholder', /Cmd\+Enter steer · Cmd\+Shift\+Enter queue/);
    await input.fill('ж?');
    await input.press('Meta+Enter');
    await expect(input).toHaveValue('');
    await expect(page.locator('.trail-user').filter({ hasText: 'ж?' })).toHaveCount(1);

    const calls = await page.evaluate(() => (window as any).__CODEX_CALLS__);
    expect(calls.filter((call: any) => call.cmd === 'steer_codex_takeover')).toHaveLength(1);
    expect(calls.filter((call: any) => call.cmd === 'send_codex_takeover_prompt')).toHaveLength(0);

    await input.fill('Queue this next.');
    await input.press('Meta+Shift+Enter');
    await expect(page.getByRole('region', { name: 'Codex prompt queue' })).toContainText('Queue this next.');
  });

  test('Given several queued prompts When Dialogue is compact Then rows stay readable and REMOVE exposes pending state', async ({ page }) => {
    await installProviderMocks(page, 'controls', true);
    await page.goto('/');
    await page.evaluate(() => {
      (window as any).__DELAY_QUEUE_REMOVE__ = true;
      (window as any).__CODEX_SNAPSHOT__.runtime = { phase: 'active', activeTurnId: 'turn-layout', error: null };
      (window as any).__CODEX_SNAPSHOT__.queue = [
        { id: 'queue-a', eckyThreadId: 'ecky-thread-1', promptText: 'First queued prompt.', status: 'queued', error: null, createdAt: 1, updatedAt: 1 },
        { id: 'queue-b', eckyThreadId: 'ecky-thread-1', promptText: 'Second queued prompt.', status: 'queued', error: null, createdAt: 2, updatedAt: 2 },
        { id: 'queue-c', eckyThreadId: 'ecky-thread-1', promptText: 'Third queued prompt.', status: 'queued', error: null, createdAt: 3, updatedAt: 3 },
        { id: 'queue-d', eckyThreadId: 'ecky-thread-1', promptText: 'Fourth queued prompt.', status: 'queued', error: null, createdAt: 4, updatedAt: 4 },
        { id: 'queue-e', eckyThreadId: 'ecky-thread-1', promptText: 'Fifth queued prompt.', status: 'queued', error: null, createdAt: 5, updatedAt: 5 },
      ];
    });
    await selectCodexProvider(page);
    await openDialogue(page);

    const queue = page.getByRole('region', { name: 'Codex prompt queue' });
    const rows = queue.locator('.codex-queue__item');
    await expect(rows).toHaveCount(5);
    const rowHeights = await rows.evaluateAll((items) => items.map((item) => item.getBoundingClientRect().height));
    expect(rowHeights.every((height) => height >= 28)).toBe(true);
    await expect.poll(() => queue.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
    const queueBox = await queue.boundingBox();
    const inputBox = await page.getByPlaceholder(/Type a question or design change/i).boundingBox();
    expect(queueBox && inputBox && queueBox.y + queueBox.height <= inputBox.y).toBe(true);

    await rows.first().getByRole('button', { name: 'REMOVE' }).click();
    const removing = rows.first().getByRole('button', { name: 'REMOVING…' });
    await expect(removing).toBeDisabled();
    await page.evaluate(() => (window as any).__RESOLVE_QUEUE_REMOVE__());
    await expect(queue).not.toContainText('First queued prompt.');
    await expect(queue).toContainText('Second queued prompt.');
  });

  test('Given owned conversation When turn start fails Then transcript and retryable FIFO remain', async ({ page }) => {
    await installProviderMocks(page, 'turnFailure', true); await bootProviderDialogue(page);
    const input = page.getByPlaceholder(/Type a question or design change/i);
    await input.fill('Cut the bearing seat.'); await page.getByRole('button', { name: 'SEND TO CODEX' }).click();
    await expect(page.getByText('turn/start failed: workspace sandbox denied write')).toBeVisible();
    await expect(page.locator('.trail-assistant').filter({ hasText: 'Wall thickness locked.' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'RETRY' })).toBeVisible();
  });

  test('Given an owned provider queue When Codex desktop has no open task Then Dialogue never requires opening Codex', async ({ page }) => {
    await installProviderMocks(page, 'controls', true); await page.goto('/'); await selectCodexProvider(page);
    await openDialogue(page);
    await page.evaluate(() => {
      (window as any).__CODEX_SNAPSHOT__.queue = [{
        id: 'queue-waiting',
        eckyThreadId: 'ecky-thread-1',
        promptText: 'Deliver after task opens.',
        status: 'queued',
        error: 'Codex desktop task is not open. Open it to deliver queued messages.',
        createdAt: 1,
        updatedAt: 1,
      }];
      (window as any).__EMIT_CODEX_EVENT__('queue/dispatched');
    });

    const queue = page.getByRole('region', { name: 'Codex prompt queue' });
    await expect(queue).toContainText('QUEUED');
    await expect(queue).toContainText('Codex desktop task is not open.');
    await expect(page.getByRole('button', { name: 'OPEN CODEX TASK' })).toHaveCount(0);
  });

  test('Given an active Ecky thread When Provider config save fails Then error is global, raw, and absent from thread dialogue', async ({ page }) => {
    await installProviderMocks(page, 'happy');
    await page.goto('/');
    await page.evaluate(() => { (window as any).__CONFIG_SAVE_ERROR__ = true; });

    await page.getByRole('button', { name: 'Settings' }).click();
    const settings = page.locator('[data-window-id="settings"]');
    await settings.getByRole('button', { name: 'PROVIDER', exact: true }).click();
    await settings.getByRole('button', { name: 'SAVE REGISTRY' }).click();

    await expect(settings.locator('.status-msg')).toContainText('invalid config field connection-type');
    const globalError = page.locator('.agent-card').filter({ hasText: 'invalid config field connection-type' });
    await expect(globalError.locator('.agent-card__thread')).toHaveText('ECKY APP');
    await expect(globalError).not.toHaveAttribute('data-thread-id');
    await expect(page.locator('.genie-bubble').filter({ hasText: 'Config Save Error' })).toHaveCount(0);
    await expect(page.locator('.genie-bubble').filter({ hasText: 'invalid config field connection-type' })).toHaveCount(0);
  });
});
