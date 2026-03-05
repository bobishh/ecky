import { test, expect } from '@playwright/test';

test.describe('Concurrency Isolation', () => {
  test('switching threads during generation does not mutate the new thread', async ({ page }) => {
    // Mock the Tauri invoke to simulate a slow generation and basic boot
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      const originalInvoke = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') {
          return { engines: [{ id: 'mock', name: 'Mock' }], selected_engine_id: 'mock' };
        }
        if (cmd === 'get_history') {
          return [{ id: 'mock-thread-2', title: 'Existing Thread', updated_at: Date.now(), version_count: 1 }];
        }
        if (cmd === 'get_thread') {
          return { id: 'mock-thread-2', title: 'Existing Thread', messages: [] };
        }
        if (cmd === 'generate_design') {
          // Artificial delay
          await new Promise(resolve => setTimeout(resolve, 1000));
          return {
            threadId: args.threadId || 'mock-thread-1',
            messageId: 'mock-msg-1',
            design: {
              interaction_mode: 'design',
              macro_code: 'print("mock")',
              initial_params: {},
              ui_spec: { fields: [] }
            }
          };
        }
        if (cmd === 'render_stl') {
          return '/mock/path/to.stl';
        }
        if (originalInvoke) {
          return originalInvoke(cmd, args);
        }
        return {};
      };
    });

    await page.goto('/');
    await page.waitForSelector('.workbench');

    // Type a prompt
    const textarea = page.locator('.prompt-input');
    await textarea.fill('Build a box');
    const sendBtn = page.locator('button:has-text("SEND")');
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
