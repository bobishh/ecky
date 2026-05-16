import { expect, test, type Page, type Route } from '@playwright/test';

const MOCK_STL = `solid genie
facet normal 0 0 1
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid genie`;

async function installGenieMocks(page: Page) {
  await page.route(/\/mock\/.*\.stl(?:\?.*)?$/, async (route: Route) => {
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: MOCK_STL,
    });
  });

  await page.addInitScript(() => {
    const snapshot = {
      design: {
        title: 'Validation Fixture',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        macroCode: '(solid validation-fixture)',
        initialParams: {},
      },
      threadId: 'thread-preview-feedback',
      messageId: 'msg-preview-feedback',
      selectedPartId: null,
      artifactBundle: {
        modelId: 'preview-feedback-model',
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        contentHash: 'preview-feedback-hash',
        artifactVersion: 1,
        manifestPath: '/mock/model-runtime/manifest.json',
        macroPath: '/mock/model-runtime/source.ecky',
        previewStlPath: '/mock/model-runtime/preview-feedback.stl',
        viewerAssets: [],
      },
      modelManifest: {
        modelId: 'preview-feedback-model',
        sourceKind: 'generated',
        engineKind: 'ecky',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        contentHash: 'preview-feedback-hash',
        artifactVersion: 1,
        manifestPath: '/mock/model-runtime/manifest.json',
        macroPath: '/mock/model-runtime/source.ecky',
        parts: [],
      },
    };

    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    window.__TAURI_INTERNALS__.invoke = async (cmd: string) => {
      if (cmd === 'get_config') {
        return {
          engines: [],
          selectedEngineId: '',
          freecadCmd: '',
          assets: [],
          microwave: { humId: null, dingId: null, muted: true },
          voice: { sttLanguageCode: 'en-US' },
          mcp: {
            port: null,
            maxSessions: null,
            mode: 'active',
            primaryAgentId: null,
            promptTimeoutSecs: 1800,
            eckyAstAuthoring: false,
            autoAgents: [],
          },
          hasSeenOnboarding: true,
          connectionType: 'mcp',
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
      if (cmd === 'get_last_design') return snapshot;
      if (cmd === 'get_thread_message_version') return null;
      if (cmd === 'get_thread_latest_version') return null;
      if (cmd === 'get_thread_messages_page') return { messages: [], nextBefore: null, hasMore: false };
      if (cmd === 'get_default_macro') return '(solid blank)';
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
          connectionState: 'active',
          agentLabel: 'Codex',
          llmModelLabel: 'gpt-5',
          providerKind: 'openai',
          sessionId: 'session-preview-feedback',
          phase: 'patching_macro',
          statusText: 'Preview validation found a containment mismatch on front profile. Repairing source bounds and rerunning exact hidden-line validation.',
          busy: true,
          activityLabel: 'Preview validation found a containment mismatch on front profile. Repairing source bounds and rerunning exact hidden-line validation.',
          activityStartedAt: 100,
          attentionKind: null,
          waitingOnPrompt: false,
          updatedAt: 100,
        };
      }
      return null;
    };
  });
}

test.describe('VertexGenie', () => {
  test('Given workbench loads When Ecky appears Then angular SVG mascot renders in the viewport', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const mascot = page.locator('.genie-corner-svg');
    await expect(mascot).toBeVisible();
    await expect(mascot.locator('.genie-corner-body')).toHaveCount(1);
    await expect(mascot.locator('.genie-corner-node')).toHaveCount(6);
  });

  test('genie is present in the genie layer', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const genieLayer = page.locator('.genie-layer');
    await expect(genieLayer).toBeVisible();
  });

  test('Given preview validation feedback When workbench opens Then bubble stays compact and clear of top controls', async ({ page }) => {
    await installGenieMocks(page);
    await page.setViewportSize({ width: 1120, height: 820 });

    await page.goto('/');

    const bubble = page.locator('.genie-bubble[data-bubble-layout="compact"]');
    await expect(bubble).toBeVisible();
    await expect(bubble.getByText('PREVIEW CHECK')).toBeVisible();

    const bubbleBox = await bubble.boundingBox();
    const controlsBox = await page.locator('.app-overlay-actions').boundingBox();

    expect(bubbleBox).not.toBeNull();
    expect(controlsBox).not.toBeNull();
    if (!bubbleBox || !controlsBox) return;

    const overlaps =
      bubbleBox.x < controlsBox.x + controlsBox.width &&
      bubbleBox.x + bubbleBox.width > controlsBox.x &&
      bubbleBox.y < controlsBox.y + controlsBox.height &&
      bubbleBox.y + bubbleBox.height > controlsBox.y;

    expect(overlaps).toBe(false);
    expect(bubbleBox.width).toBeLessThanOrEqual(360);
  });

  test('Given preview validation feedback When agent is repairing Then Ecky uses active SVG state styling', async ({ page }) => {
    await installGenieMocks(page);
    await page.goto('/');

    const mascot = page.locator('.genie-corner-svg');
    await expect(mascot).toBeVisible();
    await expect(mascot).toHaveAttribute('data-mode', 'thinking');
    await expect(mascot.locator('.genie-corner-selected-edge')).toBeVisible();
  });
});
