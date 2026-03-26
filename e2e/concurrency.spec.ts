import { test, expect } from '@playwright/test';

test.describe('Concurrency Isolation', () => {
  test('switching threads during generation does not mutate the new thread', async ({ page }) => {
    // Mock the Tauri invoke to simulate a slow generation and basic boot
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') {
          return {
            engines: [{ id: 'mock', name: 'Mock' }],
            selectedEngineId: 'mock',
            hasSeenOnboarding: true,
          };
        }
        if (cmd === 'check_freecad') return true;
        if (cmd === 'get_history') {
          return [{
            id: 'mock-thread-2',
            title: 'Existing Thread',
            updatedAt: Date.now() / 1000,
            versionCount: 1,
            pendingCount: 0,
            errorCount: 0,
            summary: '',
            messages: [],
          }];
        }
        if (cmd === 'get_last_design') return null;
        if (cmd === 'get_default_macro') return '# mock macro';
        if (cmd === 'get_thread') {
          return {
            id: args.id,
            title: 'Existing Thread',
            updatedAt: Date.now() / 1000,
            versionCount: 1,
            pendingCount: 0,
            errorCount: 0,
            summary: '',
            messages: [],
          };
        }
        if (cmd === 'generate_design') {
          // Artificial delay
          await new Promise(resolve => setTimeout(resolve, 1000));
          return {
            threadId: args.threadId || 'mock-thread-1',
            messageId: 'mock-msg-1',
            design: {
              interactionMode: 'design',
              macroCode: 'print("mock")',
              initialParams: {},
              uiSpec: { fields: [] }
            }
          };
        }
        if (cmd === 'render_stl') {
          return '/mock/path/to.stl';
        }
        if (cmd === 'save_config') return null;
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
        if (cmd === 'finalize_generation_attempt') return null;
        if (cmd === 'save_last_design') return null;
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
    });

    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);
    await page.waitForSelector('.workbench');
    await expect(page.locator('.history-card')).toContainText('Existing Thread');

    // Type a prompt
    const textarea = page.locator('.prompt-input');
    await textarea.fill('Build a box');
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await expect(sendBtn).toBeEnabled();
    await sendBtn.click();

    // Immediately click the existing thread in history
    const historyCard = page.locator('.history-card').first();
    if (await historyCard.isVisible()) {
      await historyCard.click();
    }

    // Wait for the mock generation delay
    await page.waitForTimeout(1500);

    // Assert that the generated output did not bleed into the newly selected thread view
    // i.e., the active thread should be mock-thread-2 and not mock-thread-1
    const activeCard = page.locator('.history-card.active');
    await expect(activeCard).toContainText('Existing Thread');
  });
});
