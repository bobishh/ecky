import { test, expect } from '@playwright/test';

function installProjectSwitcherMocks(options?: {
  inventory?: Array<Record<string, unknown>>;
  inventoryError?: { message: string; details?: string };
  latestVersionDelayMs?: number;
}) {
  const inventory = options?.inventory ?? [];
  const inventoryError = options?.inventoryError ?? null;
  const latestVersionDelayMs = options?.latestVersionDelayMs ?? 0;

  return async ({ page }: { page: import('@playwright/test').Page }) => {
    await page.addInitScript(
      ({ inventory, inventoryError, latestVersionDelayMs }) => {
        const mockWindow = window as any;
        localStorage.clear();
        mockWindow.__PROJECTS_CALLS__ = [];

        const config = {
          engines: [{ id: 'mock', name: 'Mock', provider: 'openai', apiKey: '', model: 'mock', baseUrl: '', enabled: true }],
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

        window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
        window.__TAURI_INTERNALS__.metadata = {};
        window.__TAURI_INTERNALS__.transformCallback = (callback: unknown) => {
          const id = Math.floor(Math.random() * 1_000_000_000);
          (window as any)[`_${id}`] = callback;
          return id;
        };
        window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
          mockWindow.__PROJECTS_CALLS__.push({ cmd, args });
          if (cmd === 'get_config') return structuredClone(config);
          if (cmd === 'save_config') return null;
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
          if (cmd === 'get_inventory') {
            if (inventoryError) {
              throw { code: 'persistence', message: inventoryError.message, details: inventoryError.details };
            }
            return structuredClone(inventory);
          }
          if (cmd === 'get_deleted_messages') return [];
          if (cmd === 'get_last_design') return null;
          if (cmd === 'get_active_agent_sessions') return [];
          if (cmd === 'get_agent_terminal_snapshots') return [];
          if (cmd === 'get_mcp_server_status') return [];
          if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
          if (cmd === 'get_default_macro') return '# mock macro';
          if (cmd === 'get_thread_latest_version') {
            if (latestVersionDelayMs > 0) {
              await new Promise((resolve) => setTimeout(resolve, latestVersionDelayMs));
            }
            return null;
          }
          if (cmd === 'get_thread_messages_page') {
            return {
              messages: [],
              hasMore: false,
              nextBefore: null,
            };
          }
          return null;
        };
      },
      { inventory, inventoryError, latestVersionDelayMs },
    );
  };
}

test.describe('History Panel', () => {
  test('shows search input', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const searchInput = page.locator('[data-window-id="projects"] .search-input');
    await expect(searchInput).toBeVisible();
    await expect(searchInput).toHaveAttribute('placeholder', 'Search...');
  });

  test('new project button opens chooser', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-window-id="projects"]').getByRole('button', { name: '+ NEW' }).click();
    await expect(page.getByRole('dialog', { name: /Start New Project/i })).toBeVisible();
  });

  test('Given blank project starts When default macro seeds thread Then viewport code opens immediately', async ({ page }) => {
    await installProjectSwitcherMocks()({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-window-id="projects"]').getByRole('button', { name: '+ NEW' }).click();
    await page.getByRole('button', { name: 'Blank Project' }).click();

    const viewportCodeButton = page.locator('.export-actions').getByRole('button', { name: /CODE/i });
    await expect(viewportCodeButton).toBeVisible();
    await expect(viewportCodeButton).toBeEnabled();

    await viewportCodeButton.click();

    await expect(page.getByText(/MACRO INSPECTOR:/i)).toBeVisible();
    await expect(page.locator('.cm-content').first()).toContainText('# mock macro');
  });

  test('shows no project cards when no history', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await expect(page.locator('[data-window-id="projects"] .project-card')).toHaveCount(0);
  });

  test.describe('Archived projects', () => {
    test.beforeEach(
      installProjectSwitcherMocks({
        inventory: [
          {
            id: 'archived-1',
            title: 'Tradescantia zebrina pot',
            summary: 'twisted wall pot',
            updatedAt: Date.UTC(2026, 4, 22),
            messages: [],
            genieTraits: null,
            versionCount: 3,
            pendingCount: 0,
            queuedCount: 0,
            errorCount: 0,
            status: 'finalized',
            finalizedAt: Date.UTC(2026, 4, 22),
            pendingConfirm: null,
          },
        ],
        latestVersionDelayMs: 3000,
      }),
    );

    test('Given slow preview metadata, when archived opens, then cards render without waiting', async ({ page }) => {
      await page.goto('/');
      await page.getByRole('button', { name: 'PROJECTS' }).click();
      await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'ARCHIVED' }).click();

      const projectsWindow = page.locator('[data-window-id="projects"]');
      await expect(projectsWindow.locator('.project-card')).toHaveCount(1);
      await expect(projectsWindow.getByText('Tradescantia zebrina pot')).toBeVisible();

      const calls = await page.evaluate(() => (window as any).__PROJECTS_CALLS__ as Array<{ cmd: string }>);
      expect(calls.filter((entry) => entry.cmd === 'get_thread_messages_page')).toHaveLength(0);
    });
  });

  test('Given inventory backend failure, when archived opens, then raw error shows', async ({ page }) => {
    await installProjectSwitcherMocks({
      inventoryError: {
        message: 'Inventory query failed',
        details: 'database is locked',
      },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'ARCHIVED' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow.getByText('ARCHIVED LOAD ERROR')).toBeVisible();
    await expect(projectsWindow.getByText('Inventory query failed')).toBeVisible();
    await expect(projectsWindow.getByText('database is locked')).toBeVisible();
  });
});
