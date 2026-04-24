import { expect, test, type Page } from '@playwright/test';

function installPromptVoiceMocks() {
  return async ({ page }: { page: Page }) => {
    await page.addInitScript(() => {
      const mockWindow = window as any;
      localStorage.clear();
      mockWindow.__VOICE_RECORDER_CALLS__ = [];
      mockWindow.__TRANSCRIBE_CALLS__ = [];
      mockWindow.__SAVE_CONFIG_CALLS__ = [];
      mockWindow.__VOICE_TRANSCRIBE_MODE__ = 'ok';
      const config = {
        engines: [{ id: 'mock', name: 'Mock', provider: 'openai', apiKey: '', model: 'mock', baseUrl: '', systemPrompt: '', enabled: true }],
        selectedEngineId: 'mock',
        freecadCmd: '',
        assets: [],
        microwave: { humId: null, dingId: null, muted: true },
        voice: { sttLanguageCode: 'en-US' },
        mcp: { mode: 'passive', autoAgents: [] },
        hasSeenOnboarding: true,
        connectionType: null,
        defaultEngineKind: 'freecad',
        defaultSourceLanguage: 'legacyPython',
        defaultGeometryBackend: 'freecad',
        maxGenerationAttempts: 3,
        maxVerifyAttempts: 0,
      };
      mockWindow.__ECKY_TEST_AUDIO_RECORDER__ = {
        start: async () => {
          mockWindow.__VOICE_RECORDER_CALLS__.push('start');
        },
        stop: async () => {
          mockWindow.__VOICE_RECORDER_CALLS__.push('stop');
          return {
            base64Data: 'UklGRgAAAAA=',
            mimeType: 'audio/wav',
          };
        },
        cancel: () => {
          mockWindow.__VOICE_RECORDER_CALLS__.push('cancel');
        },
      };

      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.metadata = {};
      window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
        const id = Math.floor(Math.random() * 1_000_000_000);
        (window as any)[`_${id}`] = callback;
        return id;
      };
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'get_config') {
          return structuredClone(config);
        }
        if (cmd === 'save_config') {
          mockWindow.__SAVE_CONFIG_CALLS__.push(args?.config ?? null);
          Object.assign(config, args?.config ?? {});
          return null;
        }
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
            mesh: { available: true, detail: 'Ready', path: '/mock/mesh' },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
            },
          };
        }
        if (cmd === 'get_history') return [];
        if (cmd === 'get_inventory') return [];
        if (cmd === 'get_deleted_messages') return [];
        if (cmd === 'get_last_design') return null;
        if (cmd === 'get_active_agent_sessions') return [];
        if (cmd === 'get_agent_terminal_snapshots') return [];
        if (cmd === 'get_mcp_server_status') return [];
        if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
        if (cmd === 'get_default_macro') return '# mock macro';
        if (cmd === 'transcribe_prompt_audio') {
          mockWindow.__TRANSCRIBE_CALLS__.push(args?.input ?? null);
          if (mockWindow.__VOICE_TRANSCRIBE_MODE__ === 'error') {
            throw {
              code: 'provider',
              message: 'NVIDIA Speech transcription failed',
              details: '401 Unauthorized: invalid API key from provider',
            };
          }
          return {
            text: 'make a quiet robot bracket',
            provider: 'nvidia-speech',
            model: 'parakeet-tdt-0.6b-v2',
          };
        }
        return null;
      };
    });
  };
}

test.describe('Push to talk', () => {
  test.beforeEach(installPromptVoiceMocks());

  test('Given prompt voice control, holding button inserts transcript', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    const voiceButton = page.getByRole('button', { name: /start voice input/i });
    await expect(voiceButton).toBeVisible();

    await voiceButton.evaluate((element) => {
      element.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, pointerId: 1 }));
    });
    await expect(page.locator('.voice-status')).toContainText('LISTENING');
    await voiceButton.evaluate((element) => {
      element.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 1 }));
    });

    await expect(page.locator('.prompt-input')).toHaveValue('make a quiet robot bracket');
    await expect(page.evaluate(() => (window as any).__VOICE_RECORDER_CALLS__)).resolves.toEqual(['start', 'stop']);
    await expect(page.evaluate(() => (window as any).__TRANSCRIBE_CALLS__[0])).resolves.toMatchObject({
      base64Data: 'UklGRgAAAAA=',
      mimeType: 'audio/wav',
    });
  });

  test('Given provider failure, raw backend detail is visible', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    await page.evaluate(() => {
      (window as any).__VOICE_TRANSCRIBE_MODE__ = 'error';
    });

    const voiceButton = page.getByRole('button', { name: /start voice input/i });
    await voiceButton.evaluate((element) => {
      element.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, pointerId: 1 }));
      element.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 1 }));
    });

    await expect(page.locator('.voice-status')).toContainText('401 Unauthorized: invalid API key from provider');
  });

  test('Given Sounds STT language, saving routes push-to-talk through that language', async ({ page }) => {
    await page.goto('/');
    await page.locator('button[title="Settings"]').click();
    await page.getByRole('button', { name: 'SOUNDS' }).click();

    const languageInput = page.getByLabel('STT LANGUAGE CODE');
    await expect(languageInput).toBeVisible();
    await expect(languageInput).toHaveValue('en-US');

    await languageInput.fill('ru-RU');
    await page.getByRole('button', { name: 'SAVE REGISTRY' }).click();
    await expect(page.locator('.status-msg')).toContainText('Registry saved successfully.');
    await expect(page.evaluate(() => (window as any).__SAVE_CONFIG_CALLS__.at(-1)?.voice)).resolves.toEqual({
      sttLanguageCode: 'ru-RU',
    });

    await page.locator('[data-window-id="settings"] .window-close').click();
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
    const voiceButton = page.getByRole('button', { name: /start voice input/i });
    await voiceButton.evaluate((element) => {
      element.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, pointerId: 1 }));
      element.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 1 }));
    });

    await expect(page.evaluate(() => (window as any).__TRANSCRIBE_CALLS__[0])).resolves.toMatchObject({
      languageCode: 'ru-RU',
    });
  });
});
