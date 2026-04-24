import { test, expect, type Page } from '@playwright/test';

type MockEngine = {
  id: string;
  name: string;
  provider: string;
  apiKey: string;
  model: string;
  lightModel: string;
  baseUrl: string;
  systemPrompt: string;
  enabled: boolean;
};

function buildConfig(model: string): {
  engines: MockEngine[];
  selectedEngineId: string;
  hasSeenOnboarding: boolean;
  freecadCmd: string;
  assets: unknown[];
  microwave: null;
  mcp: {
    port: null;
    maxSessions: null;
    mode: 'passive';
    primaryAgentId: null;
    promptTimeoutSecs: number;
    autoAgents: unknown[];
  };
  connectionType: 'api_key';
  defaultEngineKind: 'build123d';
  defaultSourceLanguage: 'ecky';
  defaultGeometryBackend: 'build123d';
  maxGenerationAttempts: number;
  maxVerifyAttempts: number;
} {
  return {
    engines: [
      {
        id: 'nim',
        name: 'NVIDIA NIM',
        provider: 'openai',
        apiKey: 'nvapi-test',
        model,
        lightModel: model,
        baseUrl: 'https://integrate.api.nvidia.com/v1',
        systemPrompt: 'test prompt',
        enabled: true,
      },
    ],
    selectedEngineId: 'nim',
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
    connectionType: 'api_key',
    defaultEngineKind: 'build123d',
    defaultSourceLanguage: 'ecky',
    defaultGeometryBackend: 'build123d',
    maxGenerationAttempts: 3,
    maxVerifyAttempts: 1,
  };
}

async function installNimMock(page: Page, model: string) {
  await page.addInitScript((mockConfig) => {
    const config = structuredClone(mockConfig);

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') return config;
      if (cmd === 'save_config') {
        Object.assign(config, args.config);
        return null;
      }
      if (cmd === 'get_system_prompt') return 'test prompt';
      if (cmd === 'list_models') {
        return [
          'meta/llama-3.1-70b-instruct',
          'microsoft/phi-4-multimodal-instruct',
        ];
      }
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: false, detail: 'missing', path: null },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: {
            engineKind: 'build123d',
            sourceLanguage: 'build123d',
            geometryBackend: 'build123d',
          },
        };
      }
      if (cmd === 'check_freecad') return false;
      if (cmd === 'get_history') return [];
      if (cmd === 'get_last_design') return null;
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
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      return null;
    };
  }, buildConfig(model));
}

async function openNimEngineSettings(page: Page) {
  await page.goto('/');
  await expect(page.getByRole('button', { name: '⚙️' })).toBeVisible();
  await page.getByRole('button', { name: '⚙️' }).click();
  await expect(page.getByText('CONNECTION TYPE')).toBeVisible();
  await expect(page.locator('.engine-card')).toHaveCount(1);
  await page.locator('.engine-card').first().click();
  await expect(page.locator('#e-baseurl')).toBeVisible();
}

test.describe('NVIDIA NIM vision capability hints', () => {
  test('Given text-only NIM model When engine settings open Then preview warning shows', async ({ page }) => {
    await installNimMock(page, 'meta/llama-3.1-70b-instruct');
    await openNimEngineSettings(page);

    await expect(
      page.getByTestId('engine-vision-warning'),
    ).toContainText(/concept-preview reuse, and screenshot verification are unavailable/i);
  });

  test('Given vision-capable NIM model When engine settings open Then preview warning stays hidden', async ({ page }) => {
    await installNimMock(page, 'microsoft/phi-4-multimodal-instruct');
    await openNimEngineSettings(page);

    await expect(page.getByTestId('engine-vision-warning')).toHaveCount(0);
  });
});
