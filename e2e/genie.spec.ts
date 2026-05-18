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

async function installGenieMocks(page: Page, agentState: Record<string, unknown> = {}) {
  await page.route(/\/mock\/.*\.stl(?:\?.*)?$/, async (route: Route) => {
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: MOCK_STL,
    });
  });

  await page.addInitScript((mockAgentState) => {
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
          ...(mockAgentState as Record<string, unknown>),
        };
      }
      return null;
    };
  }, agentState);
}

test.describe('VertexGenie', () => {
  test('Given workbench loads When Ecky appears Then Three stone mascot renders in the viewport', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const mascot = page.locator('.genie-stone-canvas');
    await expect(mascot).toBeVisible();
    await expect(page.locator('.genie-corner-svg')).toHaveCount(0);
    const nonTransparentPixels = await mascot.evaluate((canvas) => {
      const gl =
        (canvas as HTMLCanvasElement).getContext('webgl2', { preserveDrawingBuffer: true }) ||
        (canvas as HTMLCanvasElement).getContext('webgl', { preserveDrawingBuffer: true });
      if (!gl) return 0;
      const width = (canvas as HTMLCanvasElement).width;
      const height = (canvas as HTMLCanvasElement).height;
      const data = new Uint8Array(width * height * 4);
      gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, data);
      let count = 0;
      for (let index = 3; index < data.length; index += 4) {
        if (data[index] > 0) count++;
      }
      return count;
    });
    expect(nonTransparentPixels).toBeGreaterThan(2400);
  });

  test('genie is present in the genie layer', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const genieLayer = page.locator('.genie-layer');
    await expect(genieLayer).toBeVisible();
  });

  test('Given Ecky is poked repeatedly When user clicks the mascot Then it enters angry poke state', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    const mascot = page.locator('.genie-layer .genie-stone-button');
    await expect(mascot).toBeVisible();
    await expect(mascot).toHaveAttribute('data-poke-state', 'calm');

    for (let index = 0; index < 5; index++) {
      await mascot.click();
      await page.waitForTimeout(150);
    }

    await expect(mascot).toHaveAttribute('data-poke-state', 'angry');
  });

  test('Given Ecky has model DNA When user rerolls seed from settings Then mascot seed changes without poking', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    const mascot = page.locator('.genie-layer .genie-stone-button');
    await expect(mascot).toBeVisible();
    await expect(page.locator('.genie-layer [aria-label="Reroll Ecky seed"]')).toHaveCount(0);
    const before = await mascot.getAttribute('data-seed');

    await page.locator('button[title="Settings"], button[title="Configuration"]').click();
    const settingsWindow = page.locator('[data-window-id="settings"]');
    await expect(settingsWindow).toBeVisible();
    const appTab = settingsWindow.getByRole('button', { name: 'APP' });
    await expect(appTab).toBeVisible();
    await appTab.click();
    const reroll = page.getByRole('button', { name: 'Reroll Ecky seed' });
    await expect(page.getByTestId('settings-ecky-preview')).toBeVisible();
    await expect(reroll).toBeVisible();
    await reroll.click();

    await expect(mascot).not.toHaveAttribute('data-seed', before ?? '');
    await expect(mascot).toHaveAttribute('data-poke-state', 'calm');
  });

  test('Given Ecky is dragged When user rotates the mascot Then it does not count as a poke', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);

    const mascot = page.locator('.genie-stone-button');
    await expect(mascot).toBeVisible();
    await expect(mascot).toHaveAttribute('data-drag-revision', '0');

    const box = await mascot.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;

    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 36, box.y + box.height / 2 - 12, { steps: 4 });
    await page.mouse.up();

    await expect(mascot).not.toHaveAttribute('data-drag-revision', '0');
    await expect(mascot).toHaveAttribute('data-poke-state', 'calm');
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

  test('Given preview validation feedback When user opens bubble Then session activity shows full text', async ({ page }) => {
    await installGenieMocks(page);
    await page.goto('/');

    const bubble = page.getByTestId('genie-session-bubble');
    await expect(bubble).toBeVisible();

    await bubble.getByRole('button', { name: 'Copy advisor response' }).click();
    await expect(page.locator('[data-window-id="activity"]')).toHaveCount(0);

    await bubble.click();
    const activityWindow = page.locator('[data-window-id="activity"]');
    await expect(activityWindow).toBeVisible();
    await expect(activityWindow.getByTestId('activity-event-list')).toBeVisible();
    await expect(activityWindow.getByTestId('activity-event-detail')).toContainText(
      'Preview validation found a containment mismatch on front profile.',
    );
    await expect(activityWindow.getByTestId('session-preview-detail')).toContainText('preview-feedback.stl');
  });

  test('Given preview draft feedback with authoring lint When workbench opens Then bubble mentions lint suggestion', async ({ page }) => {
    await installGenieMocks(page, {
      authoringLints: [
        {
          message:
            'Repeated anonymous delta on slotWidth in part holder. Extract slot_margin_x parameter and reuse.',
        },
      ],
    });
    await page.goto('/');

    const bubble = page.locator('.genie-bubble');
    await expect(bubble).toBeVisible();
    await expect(bubble).toContainText('Authoring lint:');
    await expect(bubble).toContainText('slot_margin_x');
  });

  test('Given preview draft feedback is pending without lints When workbench opens Then bubble omits lint suggestion text', async ({ page }) => {
    await installGenieMocks(page, {
      phase: 'rendering',
      statusText: 'Draft preview pending while source updates apply.',
      activityLabel: 'Draft preview pending while source updates apply.',
      authoringLints: [],
    });
    await page.goto('/');

    const bubble = page.locator('.genie-bubble');
    await expect(bubble).toBeVisible();
    await expect(bubble).toContainText('Draft preview pending while source updates apply.');
    await expect(bubble).not.toContainText('Authoring lint:');
  });

  test('Given preview validation feedback When agent is repairing Then Ecky uses active Three state styling', async ({ page }) => {
    await installGenieMocks(page);
    await page.goto('/');

    const mascot = page.locator('.genie-stone-canvas');
    await expect(mascot).toBeVisible();
    await expect(mascot).toHaveAttribute('data-mode', 'thinking');
  });

  test('Given backend reports an agent error When workbench opens Then Ecky uses red error state', async ({ page }) => {
    await installGenieMocks(page, {
      connectionState: 'error',
      phase: 'error',
      statusText: 'Renderer failed.',
      busy: false,
      activityLabel: null,
    });
    await page.goto('/');

    const mascot = page.locator('.genie-stone-canvas');
    await expect(mascot).toBeVisible();
    await expect(mascot).toHaveAttribute('data-mode', 'error');
  });
});
