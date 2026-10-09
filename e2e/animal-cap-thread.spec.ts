import { expect, test, type Page } from '@playwright/test';

const pugThreadId = '0e018829-ebba-4137-bab9-a32e42b49fcd';

async function installPugThreadMocks(page: Page) {
  await page.addInitScript((threadId) => {
    const pugThread = {
      id: threadId,
      title: 'Pug Presta Valve Cap',
      updatedAt: Date.now() / 1000,
      versionCount: 1,
      pendingCount: 0,
      queuedCount: 0,
      errorCount: 0,
      status: 'ready',
      summary: 'Printable Presta valve cap',
      messages: [],
    };

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') {
        return {
          engines: [],
          selectedEngineId: '',
          freecadLibraryRoots: [],
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
          defaultEngineKind: 'ecky',
          defaultSourceLanguage: 'ecky',
          defaultGeometryBackend: 'mesh',
          maxGenerationAttempts: 1,
          maxVerifyAttempts: 0,
        };
      }
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: {
            engineKind: 'ecky',
            sourceLanguage: 'ecky',
            geometryBackend: 'mesh',
          },
        };
      }
      if (cmd === 'get_history') return [pugThread];
      if (cmd === 'get_thread') return { ...pugThread, id: String(args?.id ?? threadId) };
      if (cmd === 'get_thread_preview') return null;
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_default_macro') return '';
      if (cmd === 'check_freecad') return true;
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: null,
          connectionState: 'disconnected',
          sessions: [],
          primaryAgentLabel: null,
          statusText: '',
        };
      }
      if (cmd === 'list_installed_component_package_headers') return [];
      return null;
    };
  }, pugThreadId);
}

test('Given the Pug exists in history When Projects and Library open Then it is only a normal thread', async ({
  page,
}) => {
  await installPugThreadMocks(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);

  await page.getByRole('button', { name: 'PROJECTS' }).click();
  const pugProject = page.locator(`[data-project-id="${pugThreadId}"]`);
  await expect(pugProject.getByRole('heading', { name: 'Pug Presta Valve Cap' })).toBeVisible();
  await expect(pugProject.getByRole('button', { name: 'OPEN' })).toBeVisible();

  await page.getByRole('button', { name: 'PROJECTS' }).click();
  await page.getByRole('button', { name: 'LIBRARY' }).click();
  const library = page.locator('[data-window-id="library"]');
  await expect(library.getByRole('button', { name: 'COMPONENTS' })).toBeVisible();
  await expect(library.getByRole('button', { name: 'FREECAD PARTS' })).toBeVisible();
  await expect(library.getByRole('button', { name: 'CATALOG' })).toHaveCount(0);
  await expect(library.getByText('Pug Presta Valve Cap')).toHaveCount(0);
});
