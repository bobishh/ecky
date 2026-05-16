import { test, expect } from '@playwright/test';

test.describe('Concurrency Isolation', () => {
  test('switching threads during generation does not mutate the new thread', async ({ page }) => {
    await page.route(/\/mock\.stl(?:\?.*)?$/, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body: `solid mock
facet normal 0 0 0
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid mock
`,
      });
    });
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
        if (cmd === 'get_runtime_capabilities') {
          return {
            freecad: { available: true, detail: 'Ready at /mock/freecadcmd', path: '/mock/freecadcmd' },
            build123d: { available: true, detail: 'Ready at /mock/python3', path: '/mock/python3' },
            mesh: { available: true, detail: 'bundled', path: null },
            recommendedAuthoringContext: {
              engineKind: 'freecad',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
            },
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
        if (cmd === 'get_thread_latest_version') return null;
        if (cmd === 'get_thread_message_version') return null;
        if (cmd === 'get_thread_messages_page') {
          return { messages: [], nextBefore: null, hasMore: false };
        }
        if (cmd === 'generate_design') {
          // Artificial delay
          await new Promise(resolve => setTimeout(resolve, 1000));
          return {
            threadId: args.threadId || 'mock-thread-1',
            messageId: 'mock-msg-1',
            design: {
              title: 'Mock Box',
              versionName: 'V1',
              interactionMode: 'design',
              macroCode: 'print("mock")',
              sourceLanguage: 'legacyPython',
              geometryBackend: 'freecad',
              initialParams: {},
              uiSpec: { fields: [] }
            }
          };
        }
        if (cmd === 'render_model') {
          return {
            modelId: 'mock-model-1',
            sourceKind: 'generated',
            engineKind: 'freecad',
            sourceLanguage: 'legacyPython',
            geometryBackend: 'freecad',
            contentHash: 'mock-hash-1',
            fcstdPath: '/mock.FCStd',
            manifestPath: '/mock/manifest.json',
            previewStlPath: '/mock.stl',
            viewerAssets: [],
            calloutAnchors: [],
            measurementGuides: [],
            edgeTargets: [],
          };
        }
        if (cmd === 'get_model_manifest') {
          return {
            modelId: 'mock-model-1',
            sourceKind: 'generated',
            sourceLanguage: 'legacyPython',
            geometryBackend: 'freecad',
            document: {
              documentName: 'Mock Box',
              documentLabel: 'Mock Box',
              objectCount: 0,
              warnings: [],
            },
            parts: [],
            parameterGroups: [],
            controlPrimitives: [],
            controlRelations: [],
            controlViews: [],
            selectionTargets: [],
            advisories: [],
            measurementAnnotations: [],
            warnings: [],
            enrichmentState: { status: 'none', proposals: [] },
          };
        }
        if (cmd === 'verify_generated_model') {
          return {
            passed: true,
            summary: 'Checks passed.',
            issues: [],
            metrics: {
              partCount: 1,
              previewStlSizeBytes: 1024,
              totalVolume: 1000,
              totalArea: 500,
              bbox: { xMin: 0, yMin: 0, zMin: 0, xMax: 10, yMax: 10, zMax: 10 },
            },
            verifierStatus: 'ok',
            verifierSource: 'mock',
          };
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
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await expect(page.locator('.project-card')).toContainText('Existing Thread');
    await page.getByRole('button', { name: 'DIALOGUE' }).click();

    // Type a prompt
    const textarea = page.locator('.prompt-input');
    await textarea.fill('Build a box');
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await expect(sendBtn).toBeEnabled();
    await textarea.press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');

    // Immediately click the existing thread in history
    await page.getByRole('button', { name: 'PROJECTS' }).click({ force: true });
    const projectCard = page.locator('.project-card', { hasText: 'Existing Thread' }).first();
    await expect(projectCard).toBeVisible();
    await projectCard.getByRole('button', { name: 'OPEN' }).click({ force: true });

    // Wait for the mock generation delay
    await page.waitForTimeout(1500);

    // Assert that the generated output did not bleed into the newly selected thread view
    // i.e., the active thread should be mock-thread-2 and not mock-thread-1
    await page.getByRole('button', { name: 'PROJECTS' }).click({ force: true });
    await expect(page.locator('.project-card.active')).toContainText('Existing Thread');
  });
});
