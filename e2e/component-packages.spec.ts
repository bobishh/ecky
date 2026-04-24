import { expect, test, type Page } from '@playwright/test';

type PackageLibraryMockMode = 'ok' | 'error' | 'empty' | 'installError';

async function installProjectLibraryMocks(page: Page, mode: PackageLibraryMockMode) {
  await page.addInitScript((mockMode) => {
    const packageHeader = {
      schemaVersion: 1,
      packageId: 'bike-bottle-system',
      version: '0.1.0',
      displayName: 'Bike Bottle System',
      visibility: 'public',
      tags: ['bike', 'holder'],
      portTypes: [
        {
          typeId: 'dovetail',
          displayName: 'Dovetail',
          params: [{ key: 'railWidthMm', label: 'Rail width', kind: 'number', unit: 'mm' }],
          compatibleWith: ['dovetail_slot'],
          allowedMateTypes: ['insert_rail_into_slot'],
        },
        {
          typeId: 'bolt_pattern',
          displayName: 'Bolt pattern',
          params: [{ key: 'spacingMm', label: 'Spacing', kind: 'number', unit: 'mm' }],
          compatibleWith: ['bolt_pattern'],
          allowedMateTypes: ['bolt_pattern_match'],
        },
      ],
      components: [
        {
          componentId: 'bottle_cage',
          version: '0.1.0',
          displayName: 'Bottle Cage',
          params: [{ key: 'bottleDiameterMm', label: 'Bottle diameter', kind: 'number', unit: 'mm' }],
          ports: [
            { portId: 'dovetail_slot', typeId: 'dovetail', interfaces: ['slot'] },
            { portId: 'bolt_pattern', typeId: 'bolt_pattern', interfaces: ['mount'] },
          ],
        },
        {
          componentId: 'frame_rail',
          version: '0.1.0',
          displayName: 'Frame Rail',
          params: [],
          ports: [{ portId: 'dovetail_rail', typeId: 'dovetail', interfaces: ['rail'] }],
        },
      ],
      assemblies: [
        {
          assemblyId: 'rail_cage_mount',
          displayName: 'Rail Cage Mount',
          componentCount: 2,
          mateCount: 2,
          operationCount: 1,
          output: { mode: 'separateParts' },
        },
      ],
    };

    const mockWindow = window as any;
    mockWindow.__PACKAGE_HEADERS__ = mockMode === 'ok' ? [packageHeader] : [];
    mockWindow.__LAST_PACKAGE_ARCHIVE__ = null;

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === 'get_config') {
        return {
          engines: [],
          selectedEngineId: '',
          freecadCmd: '',
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
      if (cmd === 'save_config') return null;
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
      if (cmd === 'get_history') return [];
      if (cmd === 'get_last_design') return null;
      if (cmd === 'get_default_macro') return '';
      if (cmd === 'check_freecad') return true;
      if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          threadId: null,
          connectionState: 'disconnected',
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
      if (cmd === 'list_installed_component_package_headers') {
        if (mockMode === 'error') {
          throw {
            code: 'persistence',
            message: 'component library failed',
            details: 'raw package index missing',
          };
        }
        return mockWindow.__PACKAGE_HEADERS__;
      }
      if (cmd === 'install_component_package_archive') {
        mockWindow.__LAST_PACKAGE_ARCHIVE__ = args?.archivePath ?? null;
        if (mockMode === 'installError') {
          throw {
            code: 'validation',
            message: 'package install failed',
            details: 'raw invalid package manifest',
          };
        }
        mockWindow.__PACKAGE_HEADERS__ = [packageHeader];
        return {
          header: packageHeader,
          packageDir: '/mock/component-library/bike-bottle-system/0.1.0',
        };
      }
      if (cmd === 'plugin:dialog|open') {
        return '/mock/bike-bottle-system.ecky';
      }
      return null;
    };
  }, mode);
}

test.describe('Component package library', () => {
  test('lists installed package interfaces in the Projects window', async ({ page }) => {
    await installProjectLibraryMocks(page, 'ok');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();

    await expect(page.getByText('Bike Bottle System')).toBeVisible();
    await expect(page.getByText('bike-bottle-system / 0.1.0')).toBeVisible();
    await expect(page.getByText('2 components')).toBeVisible();
    await expect(page.getByText('2 port types')).toBeVisible();
    await expect(page.getByText('Bottle Cage')).toBeVisible();
    await expect(page.getByText('dovetail_slot')).toBeVisible();
    await expect(page.getByText('Rail Cage Mount')).toBeVisible();
  });

  test('shows raw backend error when package library load fails', async ({ page }) => {
    await installProjectLibraryMocks(page, 'error');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();

    await expect(page.getByText('component library failed')).toBeVisible();
    await expect(page.getByText('raw package index missing')).toBeVisible();
  });

  test('imports a package archive and refreshes the installed list', async ({ page }) => {
    await installProjectLibraryMocks(page, 'empty');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();
    await expect(page.getByText('NO PACKAGES INSTALLED')).toBeVisible();

    await page.getByRole('button', { name: 'IMPORT PACKAGE' }).click();

    await expect(page.getByText('Bike Bottle System')).toBeVisible();
    await expect(page.getByText('bike-bottle-system / 0.1.0')).toBeVisible();
    await expect(page.evaluate(() => (window as any).__LAST_PACKAGE_ARCHIVE__)).resolves.toBe('/mock/bike-bottle-system.ecky');
  });

  test('shows raw backend error when package import fails', async ({ page }) => {
    await installProjectLibraryMocks(page, 'installError');
    await page.goto('/');
    await expect(page.locator('.boot-overlay')).toHaveCount(0);

    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.getByRole('button', { name: 'PACKAGES' }).click();
    await page.getByRole('button', { name: 'IMPORT PACKAGE' }).click();

    await expect(page.getByText('package install failed')).toBeVisible();
    await expect(page.getByText('raw invalid package manifest')).toBeVisible();
  });
});
