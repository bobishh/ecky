import { expect, test, type Page } from '@playwright/test';

type MockOptions = {
  queueFails?: boolean;
};

async function installPassiveThreadAgentMock(page: Page, options: MockOptions = {}) {
  await page.addInitScript((mockOptions: MockOptions) => {
    const now = Math.floor(Date.now() / 1000);
    const calls = { queue: 0, generate: 0 };
    const thread = {
      id: 'thread-1',
      title: 'Passive MCP Thread',
      summary: 'Thread controlled by external agent.',
      messages: [],
      updatedAt: now,
      versionCount: 0,
      pendingCount: 0,
      queuedCount: 0,
      errorCount: 0,
      genieTraits: null,
      status: 'active',
      finalizedAt: null,
      pendingConfirm: null,
      engineKind: 'ecky',
      sourceLanguage: 'ecky',
      geometryBackend: 'build123d',
    };

    (window as Window & typeof globalThis & {
      __MOCK_AGENT_DIALOGUE_CALLS__?: typeof calls;
    }).__MOCK_AGENT_DIALOGUE_CALLS__ = calls;

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
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
      if (cmd === 'get_history') return [thread];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_thread') return thread;
      if (cmd === 'get_thread_latest_version') return null;
      if (cmd === 'get_thread_messages_page') {
        return { messages: [], nextBefore: null, hasMore: false };
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
        if (mockOptions.queueFails) {
          throw new Error('queue exploded');
        }
        return { threadId: 'thread-1', messageId: 'queued-1' };
      }
      if (cmd === 'generate_design') {
        calls.generate += 1;
        throw new Error('generate path should stay unused');
      }
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      return null;
    };
  }, options);
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
  const startX = box.x + box.width * 0.78;
  const startY = box.y + box.height * 0.68;
  const endX = startX + 70;
  const endY = startY + 40;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(endX, endY);
  await page.mouse.up();
}

test.describe('Dialogue routes passive thread-owned MCP threads through queue mode', () => {
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
});
