import { test, expect } from '@playwright/test';

test.describe('Q&A and Design Flow (Mocked)', () => {
  function setupMocks(page) {
    return page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__MOCK_THREADS__ = {};
      window.__MOCK_HISTORY__ = [];

      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        console.log('[MOCK] Invoke:', cmd, args);
        if (cmd === 'get_config') return { engines: [{ id: 'mock', name: 'Mock' }], selectedEngineId: 'mock' };
        if (cmd === 'get_history') return window.__MOCK_HISTORY__;
        if (cmd === 'get_default_macro') return '# macro';
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        
        if (cmd === 'init_generation_attempt') {
          const assistantId = 'msg-' + Math.random();
          const threadId = args.threadId;
          
          if (!window.__MOCK_THREADS__[threadId]) {
            window.__MOCK_HISTORY__.unshift({
              id: threadId,
              title: args.prompt.slice(0, 20),
              updatedAt: Date.now() / 1000,
              versionCount: 0,
              pendingCount: 1,
              errorCount: 0
            });
            window.__MOCK_THREADS__[threadId] = { 
              id: threadId,
              messages: [
                { id: 'user-1', role: 'user', content: args.prompt, status: 'success', timestamp: Date.now() },
                { id: assistantId, role: 'assistant', content: 'Generating...', status: 'pending', timestamp: Date.now() + 100 }
              ] 
            };
          }
          return assistantId;
        }

        if (cmd === 'classify_intent') {
          const isQuestion = args.prompt.includes('?');
          return { 
            intentMode: isQuestion ? 'question' : 'design', 
            confidence: 1.0, 
            response: isQuestion ? 'I am a helpful assistant.' : 'Creating design.' 
          };
        }

        if (cmd === 'generate_design') {
          return {
            threadId: args.threadId,
            messageId: 'msg-final',
            design: {
              title: 'A Box',
              macroCode: 'create_box()',
              initialParams: { size: 10 },
              uiSpec: { fields: [] },
              response: 'Box created.',
              interactionMode: 'design'
            }
          };
        }

        if (cmd === 'render_stl') return '/mock/output.stl';

        if (cmd === 'finalize_generation_attempt') {
          const threadId = Object.keys(window.__MOCK_THREADS__)[0];
          const thread = window.__MOCK_THREADS__[threadId];
          if (thread) {
            const assistantMsg = thread.messages.find(m => m.role === 'assistant');
            if (assistantMsg) {
              assistantMsg.status = args.status;
              assistantMsg.content = args.responseText || (args.status === 'success' ? 'Success' : 'Error');
              if (args.design) assistantMsg.output = args.design;
            }
          }
          if (args.status === 'success') {
            window.__MOCK_HISTORY__[0].pendingCount = 0;
            if (args.design) {
              window.__MOCK_HISTORY__[0].versionCount = 1;
              window.__MOCK_HISTORY__[0].title = args.design.title;
            }
          }
          return {};
        }

        if (cmd === 'get_thread') {
          return window.__MOCK_THREADS__[args.id] || { id: args.id, messages: [] };
        }
        return {};
      };
    });
  }

  test('asking a question should show Ecky response without creating design', async ({ page }) => {
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    const textarea = page.locator('.prompt-input');
    await textarea.fill('How does this work?');
    
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await sendBtn.click();

    await page.waitForSelector('.microwave-unit', { timeout: 5000 });
    await page.waitForSelector('.mw-thinking-result', { timeout: 5000 });

    const bubbleText = page.locator('.bubble-text');
    await expect(bubbleText).toBeVisible();
    await expect(bubbleText).toContainText('I am a helpful assistant');
  });

  test('requesting a design should trigger rendering and model update', async ({ page }) => {
    page.on('console', msg => console.log(`[PAGE] ${msg.type()}: ${msg.text()}`));
    await setupMocks(page);
    await page.goto('/');
    await page.waitForSelector('.workbench');

    const textarea = page.locator('.prompt-input');
    await textarea.fill('Create a box');
    await page.click('button:has-text("PROCESS")');

    await page.waitForSelector('.microwave-unit', { timeout: 5000 });
    await page.waitForSelector('.mw-success', { timeout: 10000 });

    await expect(page.locator('.dialogue-header')).toContainText('A Box');
  });
});
