import { test, expect } from '@playwright/test';

function installProjectSwitcherMocks(options?: {
  history?: Array<Record<string, unknown>>;
  inventory?: Array<Record<string, unknown>>;
  inventoryError?: { message: string; details?: string };
  deletedProjectPages?: Record<string, {
    items: Array<Record<string, unknown>>;
    nextBefore: string | null;
    hasMore: boolean;
  }>;
  latestVersions?: Record<string, Record<string, unknown> | null>;
  messagePages?: Record<string, Array<Record<string, unknown>>>;
  workspaceProjectionDelayMs?: number;
  threadPreviews?: Record<string, string | null>;
  threadPreviewDelayMs?: number;
  campaignRuns?: Array<Record<string, unknown>>;
  campaignStep?: Record<string, unknown> | null;
  campaignTransitionResult?: Record<string, unknown> | null;
  campaignTransitionError?: string | null;
  campaignTransitionDelayMs?: number;
  runtimeFileDelay?: { includes: string; ms: number };
  bootThreadId?: string | null;
}) {
  const history = options?.history ?? [];
  const inventory = options?.inventory ?? [];
  const inventoryError = options?.inventoryError ?? null;
  const deletedProjectPages = options?.deletedProjectPages ?? {};
  const latestVersions = options?.latestVersions ?? {};
  const messagePages = options?.messagePages ?? {};
  const workspaceProjectionDelayMs = options?.workspaceProjectionDelayMs ?? 0;
  const threadPreviews = options?.threadPreviews ?? {};
  const threadPreviewDelayMs = options?.threadPreviewDelayMs ?? 0;
  const campaignRuns = options?.campaignRuns ?? [];
  const campaignStep = options?.campaignStep ?? null;
  const campaignTransitionResult = options?.campaignTransitionResult ?? null;
  const campaignTransitionError = options?.campaignTransitionError ?? null;
  const campaignTransitionDelayMs = options?.campaignTransitionDelayMs ?? 0;
  const runtimeFileDelay = options?.runtimeFileDelay ?? null;
  const bootThreadId = options?.bootThreadId ?? null;

  return async ({ page }: { page: import('@playwright/test').Page }) => {
    await page.addInitScript(
      ({
        history,
        inventory,
        inventoryError,
        deletedProjectPages,
        latestVersions,
        messagePages,
        workspaceProjectionDelayMs,
        threadPreviews,
        threadPreviewDelayMs,
        campaignRuns,
        campaignStep,
        campaignTransitionResult,
        campaignTransitionError,
        campaignTransitionDelayMs,
        runtimeFileDelay,
        bootThreadId,
      }) => {
        const mockWindow = window as any;
        localStorage.clear();
        mockWindow.__PROJECTS_CALLS__ = [];
        let mutableHistory = structuredClone(history);
        let mutableInventory = structuredClone(inventory);
        let mutableDeletedProjectPages = structuredClone(deletedProjectPages);

        const messagesPageFor = (threadId: string) => ({
          messages: structuredClone(messagePages[threadId] ?? []),
          nextBefore: null,
          hasMore: false,
          observedBytes: 0,
          truncatedFields: [],
        });
        const workspaceFor = (threadId: string, completed = false) => {
          const source = completed ? mutableInventory : mutableHistory;
          const thread = source.find((item: any) => item.id === threadId);
          if (!thread) return null;
          const selectedVersion = structuredClone(latestVersions[threadId] ?? null);
          const page = messagesPageFor(threadId);
          if (selectedVersion && !page.messages.some((message: any) => message.id === selectedVersion.id)) {
            page.messages.push(selectedVersion);
          }
          return {
            thread: { ...structuredClone(thread), messages: [] },
            messagesPage: page,
            selectedVersion,
            requestedMessageFound: false,
          };
        };

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
          if (cmd === 'get_boot_projection') {
            return {
              config: structuredClone(config),
              history: structuredClone(mutableHistory),
              workspace: bootThreadId ? workspaceFor(bootThreadId) : null,
              selectedPartId: null,
            };
          }
          if (cmd === 'get_config') return structuredClone(config);
          if (cmd === 'get_agent_activity') {
            return {
              events: [],
              latestCursor: 0,
              oldestCursor: 0,
              hasMore: false,
              droppedCount: 0,
              retainedBytes: 0,
            };
          }
          if (cmd === 'list_capture_runs') return [];
          if (cmd === 'list_campaign_runs') return structuredClone(campaignRuns);
          if (cmd === 'list_campaign_definitions') return [];
          if (cmd === 'get_campaign_step') return structuredClone(campaignStep);
          if (cmd === 'open_campaign_project') {
            const intent = args?.intent as { kind?: string; runId?: string } | undefined;
            const run = intent?.runId
              ? campaignRuns.find((candidate: any) => candidate.id === intent.runId)
              : campaignRuns[0];
            return run && campaignStep
              ? { run: structuredClone(run), step: structuredClone(campaignStep) }
              : null;
          }
          if (cmd === 'save_active_project_navigation') return structuredClone(args?.navigation ?? null);
          if (cmd === 'get_app_window_layout') return null;
          if (cmd === 'transition_campaign_run') {
            if (campaignTransitionDelayMs > 0) {
              await new Promise((resolve) => setTimeout(resolve, campaignTransitionDelayMs));
            }
            if (campaignTransitionError) {
              throw { code: 'validation', message: campaignTransitionError };
            }
            return structuredClone(campaignTransitionResult);
          }
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
          if (cmd === 'list_installed_component_package_headers') return [];
          if (cmd === 'get_history') return structuredClone(mutableHistory);
          if (cmd === 'get_workspace_projection') {
            if (workspaceProjectionDelayMs > 0) {
              await new Promise((resolve) => setTimeout(resolve, workspaceProjectionDelayMs));
            }
            return structuredClone(workspaceFor(String(args?.threadId ?? '')));
          }
          if (cmd === 'get_inventory') {
            if (inventoryError) {
              throw { code: 'persistence', message: inventoryError.message, details: inventoryError.details };
            }
            return structuredClone(mutableInventory);
          }
          if (cmd === 'delete_thread_intent') {
            const input = args?.input as { threadId?: string } | undefined;
            const id = String(input?.threadId ?? '');
            mutableHistory = mutableHistory.filter((item: any) => item.id !== id);
            return { threadId: id, history: structuredClone(mutableHistory) };
          }
          if (cmd === 'finalize_thread_intent') {
            const input = args?.input as { threadId?: string } | undefined;
            const id = String(input?.threadId ?? '');
            const project = mutableHistory.find((item: any) => item.id === id);
            if (project) {
              mutableHistory = mutableHistory.filter((item: any) => item.id !== id);
              mutableInventory = [
                {
                  ...project,
                  status: 'finalized',
                  finalizedAt: Date.UTC(2026, 6, 25),
                },
                ...mutableInventory.filter((item: any) => item.id !== id),
              ];
            }
            return { threadId: id, history: structuredClone(mutableHistory) };
          }
          if (cmd === 'reopen_thread_intent') {
            const input = args?.input as { threadId?: string } | undefined;
            const id = String(input?.threadId ?? '');
            const project = mutableInventory.find((item: any) => item.id === id);
            if (project) {
              mutableInventory = mutableInventory.filter((item: any) => item.id !== id);
              mutableHistory = [
                { ...project, status: 'active', finalizedAt: null },
                ...mutableHistory.filter((item: any) => item.id !== id),
              ];
            }
            return { threadId: id, history: structuredClone(mutableHistory) };
          }
          if (cmd === 'open_inventory_thread_intent') {
            const input = args?.input as { threadId?: string } | undefined;
            return structuredClone(workspaceFor(String(input?.threadId ?? ''), true));
          }
          if (cmd === 'get_deleted_messages') return [];
          if (cmd === 'get_deleted_threads_page') {
            const cursor = String(args?.before ?? 'first');
            return structuredClone(
              mutableDeletedProjectPages[cursor] ?? {
                items: [],
                nextBefore: null,
                hasMore: false,
              },
            );
          }
          if (cmd === 'restore_deleted_thread') {
            const id = String(args?.id ?? '');
            for (const page of Object.values(mutableDeletedProjectPages) as any[]) {
              const project = page.items.find((item: any) => item.id === id);
              if (!project) continue;
              page.items = page.items.filter((item: any) => item.id !== id);
              mutableHistory = [
                {
                  id: project.id,
                  title: project.title,
                  summary: project.summary,
                  updatedAt: project.updatedAt,
                  messages: [],
                  versionCount: project.versionCount,
                  pendingCount: 0,
                  queuedCount: 0,
                  errorCount: 0,
                  status: 'active',
                  finalizedAt: null,
                  pendingConfirm: null,
                },
                ...mutableHistory,
              ];
              break;
            }
            return null;
          }
          if (cmd === 'get_deleted_thread_preview') return null;
          if (cmd === 'get_thread_preview') {
            if (threadPreviewDelayMs > 0) {
              await new Promise((resolve) => setTimeout(resolve, threadPreviewDelayMs));
            }
            return structuredClone(threadPreviews[String(args?.id ?? '')] ?? null);
          }
          if (cmd === 'get_last_design') return null;
          if (cmd === 'get_active_agent_sessions') return [];
          if (cmd === 'get_agent_terminal_snapshots') return [];
          if (cmd === 'get_mcp_server_status') return [];
          if (cmd === 'get_mess_stl_path') return '/mock/mess.stl';
          if (cmd === 'get_default_macro') return '# mock macro';
          if (cmd === 'plugin:fs|exists') {
            const path = String(args?.path ?? '');
            if (runtimeFileDelay && path.includes(runtimeFileDelay.includes)) {
              await new Promise((resolve) => setTimeout(resolve, runtimeFileDelay.ms));
            }
            return true;
          }
          if (cmd === 'plugin:fs|size') return 1024;
          if (cmd === 'get_thread_latest_version') {
            return structuredClone(latestVersions[String(args?.threadId ?? '')] ?? null);
          }
          if (cmd === 'get_thread_messages_page') {
            const threadMessages = messagePages[String(args?.threadId ?? '')];
            if (threadMessages) {
              return {
                messages: structuredClone(threadMessages),
                hasMore: false,
                nextBefore: null,
              };
            }
            return {
              messages: [],
              hasMore: false,
              nextBefore: null,
            };
          }
          return null;
        };
      },
      {
        history,
        inventory,
        inventoryError,
        deletedProjectPages,
        latestVersions,
        messagePages,
        workspaceProjectionDelayMs,
        threadPreviews,
        threadPreviewDelayMs,
        campaignRuns,
        campaignStep,
        campaignTransitionResult,
        campaignTransitionError,
        campaignTransitionDelayMs,
        runtimeFileDelay,
        bootThreadId,
      },
    );
  };
}

test.describe('Projects', () => {
  test('Given active campaign step When learner continues Then one Rust transition owns progress persistence', async ({ page }) => {
    const run = {
      id: 'campaign-run-transition',
      kind: 'campaignRun',
      title: 'Atomic campaign',
      definitionId: 'ecky-ir-build-missions',
      definitionVersion: 'sha256:test',
      currentStepId: 'mission-01/intro',
      completedStepIds: [],
      passedChallengeIds: [],
      draftOverridesByStepId: {},
      createdAt: 1,
      updatedAt: 1,
    };
    const nextRun = {
      ...run,
      currentStepId: 'mission-01/worked',
      completedStepIds: ['mission-01/intro'],
      updatedAt: 2,
    };
    const currentStep = {
      definitionId: run.definitionId,
      definitionVersion: run.definitionVersion,
      currentStep: {
        id: run.currentStepId,
        title: 'Introduction',
        kind: 'explain',
        prose: 'Learn one bounded concept.',
        source: null,
        canonicalSourceDigest: null,
        canonicalPreview: null,
        acceptance: null,
        nextStepId: nextRun.currentStepId,
        previousStep: null,
        missionIndex: 1,
        missionCount: 1,
        stepIndex: 1,
        stepCount: 2,
      },
    };
    const nextStep = {
      ...currentStep,
      currentStep: {
        ...currentStep.currentStep,
        id: nextRun.currentStepId,
        title: 'Worked model',
        kind: 'worked',
        stepIndex: 2,
        nextStepId: null,
        previousStep: { id: run.currentStepId },
      },
    };
    await installProjectSwitcherMocks({
      campaignRuns: [run],
      campaignStep: currentStep,
      campaignTransitionResult: { run: nextRun, step: nextStep, check: null },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const projectsWindow = page.locator('[data-window-id="projects"]');
    await projectsWindow.getByRole('button', { name: 'CAMPAIGNS' }).click();
    await projectsWindow.getByRole('button', { name: 'RESUME' }).click();
    await page.getByRole('button', { name: 'CONTINUE' }).click();

    await expect.poll(() => page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (entry: { cmd: string }) => entry.cmd === 'transition_campaign_run',
      ),
    )).toEqual([{
      cmd: 'transition_campaign_run',
      args: { input: { runId: run.id, action: { action: 'continue', draft: null } } },
    }]);
    await expect(page.getByRole('heading', { name: 'Worked model' })).toBeVisible();
    await expect.poll(() => page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (entry: { cmd: string }) => entry.cmd === 'save_campaign_run',
      ).length,
    )).toBe(0);
  });

  test('Given campaign transition is pending then rejected When learner continues Then duplicate action is blocked and raw error remains', async ({ page }) => {
    const run = {
      id: 'campaign-run-failure', kind: 'campaignRun', title: 'Atomic campaign',
      definitionId: 'ecky-ir-build-missions', definitionVersion: 'sha256:test',
      currentStepId: 'mission-01/intro', completedStepIds: [], passedChallengeIds: [],
      draftOverridesByStepId: {}, createdAt: 1, updatedAt: 1,
    };
    const currentStep = {
      definitionId: run.definitionId,
      definitionVersion: run.definitionVersion,
      currentStep: {
        id: run.currentStepId, title: 'Introduction', kind: 'explain',
        prose: 'Learn one bounded concept.', source: null, canonicalSourceDigest: null,
        canonicalPreview: null, acceptance: null, nextStepId: 'mission-01/worked',
        previousStep: null, missionIndex: 1, missionCount: 1, stepIndex: 1, stepCount: 2,
      },
    };
    await installProjectSwitcherMocks({
      campaignRuns: [run],
      campaignStep: currentStep,
      campaignTransitionDelayMs: 400,
      campaignTransitionError: 'campaign transition rejected: raw backend body',
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const projectsWindow = page.locator('[data-window-id="projects"]');
    await projectsWindow.getByRole('button', { name: 'CAMPAIGNS' }).click();
    await projectsWindow.getByRole('button', { name: 'RESUME' }).click();
    await page.getByRole('button', { name: 'CONTINUE' }).click();

    await expect(page.getByRole('button', { name: 'CONTINUING…' })).toBeDisabled();
    await expect(page.getByText('campaign transition rejected: raw backend body')).toBeVisible();
    await expect.poll(() => page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (entry: { cmd: string }) => entry.cmd === 'transition_campaign_run',
      ).length,
    )).toBe(1);
    await expect(page.getByRole('heading', { name: 'Introduction' })).toBeVisible();
  });

  test('Given campaign projects When Campaigns opens Then each run shows its current mission preview', async ({ page }) => {
    await installProjectSwitcherMocks({
      campaignRuns: [
        {
          id: 'campaign-run-1',
          kind: 'campaignRun',
          title: 'Bottle cage mission',
          definitionId: 'ecky-ir-build-missions',
          definitionVersion: 'sha256:test',
          currentStepId: 'mission-02-bottle-cage-dovetail/clamp',
          completedStepIds: [],
          passedChallengeIds: [],
          draftOverridesByStepId: {},
          createdAt: Date.UTC(2026, 7, 2) / 1000,
          updatedAt: Date.UTC(2026, 7, 2) / 1000,
        },
        {
          id: 'campaign-run-stale',
          kind: 'campaignRun',
          title: 'Stale campaign',
          definitionId: 'retired-definition',
          definitionVersion: 'sha256:stale',
          currentStepId: 'retired-mission/missing',
          completedStepIds: [],
          passedChallengeIds: [],
          draftOverridesByStepId: {},
          createdAt: Date.UTC(2026, 7, 2) / 1000,
          updatedAt: Date.UTC(2026, 7, 2) / 1000,
        },
      ],
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const projectsWindow = page.locator('[data-window-id="projects"]');
    await projectsWindow.getByRole('button', { name: 'CAMPAIGNS' }).click();

    const definitionPreview = projectsWindow.getByAltText('Ecky IR campaign preview');
    const runPreview = projectsWindow.getByAltText('Bottle cage mission preview');
    await expect(definitionPreview).toBeVisible();
    await expect(runPreview).toBeVisible();
    await expect(definitionPreview).toHaveAttribute('src', /\/docs\/assets\/corner-bracket\.png$/);
    await expect(runPreview).toHaveAttribute('src', /\/docs\/assets\/dovetail-fit\.png$/);
    await expect.poll(() => definitionPreview.evaluate((image) => (image as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
    const staleCard = projectsWindow.locator('[data-project-id="campaign-run-stale"]');
    await expect(staleCard.locator('[data-preview-state="error"]')).toContainText('PREVIEW UNAVAILABLE');
  });

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

  test('Given blank project starts When code window opens Then editor stays empty', async ({ page }) => {
    await installProjectSwitcherMocks()({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-window-id="projects"]').getByRole('button', { name: '+ NEW' }).click();
    await page.getByRole('button', { name: 'Blank Project' }).click();

    const viewportCodeButton = page.getByTestId('workbench-bottom-dock').getByRole('button', { name: /CODE/i });
    await expect(viewportCodeButton).toBeVisible();
    await expect(viewportCodeButton).toBeEnabled();

    await viewportCodeButton.click();

    await expect(page.getByText(/MACRO INSPECTOR:/i)).toBeVisible();
    const editor = page.locator('.cm-content').first();
    await expect(editor).toBeVisible();
    await expect.poll(async () => (await editor.innerText()).trim()).toBe('');
    await expect(editor).not.toContainText('# mock macro');
  });

  test('shows no project cards when no history', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await expect(page.locator('[data-window-id="projects"] .project-card')).toHaveCount(0);
  });

  test('Given history changed after boot When Projects opens Then active projects refresh', async ({ page }) => {
    await installProjectSwitcherMocks({
      history: [
        {
          id: 'existing-project',
          title: 'Existing project',
          summary: '',
          messages: [],
          updatedAt: 200,
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          isBlank: false,
        },
      ],
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const getHistoryCalls = page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter((call: { cmd: string }) => call.cmd === 'get_history').length,
    );
    await expect(getHistoryCalls).resolves.toBe(1);
    await expect(page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (call: { cmd: string }) => call.cmd === 'get_boot_projection',
      ).length,
    )).resolves.toBe(1);
    await expect(page.locator('[data-window-id="projects"] [data-project-id="existing-project"]')).toBeVisible();
  });

  test('Given cached projects When switching while cache validation is pending Then loaded viewport never flashes empty', async ({ page }) => {
    const makeVersion = (threadId: string, modelId: string, path: string) => ({
      id: `${threadId}-version`,
      role: 'assistant',
      content: modelId,
      status: 'success',
      timestamp: 100,
      output: {
        title: modelId,
        versionName: 'V1',
        response: '',
        interactionMode: 'design',
        macroCode: `(model (part ${modelId} (box 1 1 1)))`,
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        engineKind: 'ecky',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      },
      artifactBundle: {
        modelId,
        sourceKind: 'generated',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        engineKind: 'ecky',
        contentHash: `${modelId}-hash`,
        artifactVersion: 1,
        modelStlPath: path,
        viewerAssets: [],
      },
      modelManifest: {
        modelId,
        sourceKind: 'generated',
        document: { documentName: modelId, documentLabel: modelId, objectCount: 1, warnings: [] },
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
      },
    });
    const alphaVersion = makeVersion('alpha', 'alpha-model', '/mock/alpha/model.stl');
    const betaVersion = makeVersion('beta', 'beta-model', '/mock/beta/model.stl');
    const makeThread = (id: string, title: string, version: Record<string, unknown>) => ({
      id,
      title,
      summary: '',
      messages: [version],
      updatedAt: id === 'alpha' ? 200 : 100,
      versionCount: 1,
      pendingCount: 0,
      queuedCount: 0,
      errorCount: 0,
      status: 'active',
      isBlank: false,
    });
    await installProjectSwitcherMocks({
      history: [makeThread('alpha', 'Alpha project', alphaVersion), makeThread('beta', 'Beta project', betaVersion)],
      latestVersions: { alpha: alphaVersion, beta: betaVersion },
      messagePages: { alpha: [alphaVersion], beta: [betaVersion] },
      workspaceProjectionDelayMs: 700,
      bootThreadId: 'alpha',
    })({ page });
    let betaStlRequests = 0;
    await page.route(/\/mock\/(alpha|beta)\/model\.stl(?:\?.*)?$/, (route) => {
      if (route.request().url().includes('/beta/')) betaStlRequests += 1;
      return route.fulfill({
        status: 200,
        contentType: 'model/stl',
        body: 'solid mock\nfacet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid mock',
      });
    });

    await page.goto('/');
    const viewer = page.locator('.viewer-host').first();
    await expect(viewer).toHaveAttribute('data-model-status', 'loaded');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-project-id="beta"]').getByRole('button', { name: 'OPEN' }).click();
    await expect.poll(() => page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (call: { cmd: string; args?: { threadId?: string } }) =>
          call.cmd === 'get_workspace_projection' && call.args?.threadId === 'beta',
      ).length,
    )).toBe(1);
    await expect(viewer).toHaveAttribute('data-model-status', 'loaded');
    await expect(page.locator('.viewer-shell')).toHaveAttribute('data-model-key', /beta-model/);
    expect(betaStlRequests).toBe(1);
    await expect(page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter((call: { cmd: string }) => call.cmd === 'render_model').length,
    )).resolves.toBe(0);
  });

  test('Given a project has no saved preview When it opens Then thread navigation never renders it', async ({ page }) => {
    const version = {
      id: 'no-preview-version',
      role: 'assistant',
      content: 'No preview model',
      status: 'success',
      timestamp: 100,
      output: {
        title: 'No preview model',
        versionName: 'V1',
        response: '',
        interactionMode: 'design',
        macroCode: '(model (part body (box 1 1 1)))',
        sourceLanguage: 'ecky',
        geometryBackend: 'mesh',
        engineKind: 'ecky',
        uiSpec: { fields: [] },
        initialParams: {},
        postProcessing: null,
      },
      artifactBundle: null,
      modelManifest: null,
    };
    await installProjectSwitcherMocks({
      history: [{
        id: 'no-preview-thread',
        title: 'No preview project',
        summary: '',
        messages: [],
        updatedAt: 100,
        versionCount: 1,
        pendingCount: 0,
        queuedCount: 0,
        errorCount: 0,
        status: 'active',
        isBlank: false,
      }],
      latestVersions: { 'no-preview-thread': version },
      messagePages: { 'no-preview-thread': [version] },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-project-id="no-preview-thread"]').getByRole('button', { name: 'OPEN' }).click();

    await expect.poll(() => page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (call: { cmd: string }) => call.cmd === 'get_workspace_projection',
      ).length,
    )).toBeGreaterThan(0);
    await expect(page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (call: { cmd: string }) => call.cmd === 'get_thread_latest_version',
      ).length,
    )).resolves.toBe(0);
    await expect.poll(() => page.evaluate(() =>
      (window as any).__PROJECTS_CALLS__.filter(
        (call: { cmd: string }) => call.cmd === 'render_model',
      ).length,
    )).toBe(0);
    await expect(page.locator('.viewport-transmutation')).toHaveCount(0);
  });

  test('Given a reusable blank, provider-only thread, and authored thread When Projects opens Then only the completely blank thread is omitted', async ({ page }) => {
    await installProjectSwitcherMocks({
      history: [
        {
          id: 'blank-thread',
          title: 'Untitled design',
          summary: 'Thread: Untitled design',
          messages: [],
          updatedAt: 200,
          versionCount: 0,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          isBlank: true,
        },
        {
          id: 'provider-only-thread',
          title: 'Draw an engine concept',
          summary: 'Provider conversation without a CAD version',
          messages: [],
          updatedAt: 150,
          versionCount: 0,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          isBlank: false,
        },
        {
          id: 'authored-thread',
          title: 'Rocksteady AirTag head',
          summary: 'Head crop and AirTag enclosure',
          messages: [],
          updatedAt: 100,
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          isBlank: false,
        },
      ],
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const projects = page.locator('[data-window-id="projects"]');
    await expect(projects.getByText('Rocksteady AirTag head', { exact: true })).toBeVisible();
    await expect(projects.getByText('Draw an engine concept', { exact: true })).toBeVisible();
    await expect(projects.getByText('Untitled design', { exact: true })).toHaveCount(0);
  });

  test('Given many projects When Projects opens Then only visible cards request lightweight previews', async ({ page }) => {
    await installProjectSwitcherMocks({
      history: Array.from({ length: 40 }, (_, index) => ({
        id: `thread-${index + 1}`,
        title: `Preview thread ${index + 1}`,
        summary: '',
        updatedAt: Date.UTC(2026, 5, index + 1),
        messages: [],
        genieTraits: null,
        versionCount: 1,
        pendingCount: 0,
        queuedCount: 0,
        errorCount: 0,
        status: 'ready',
        finalizedAt: null,
        pendingConfirm: null,
      })),
      threadPreviews: Object.fromEntries(
        Array.from({ length: 40 }, (_, index) => [
          `thread-${index + 1}`,
          `data:image/png;base64,${btoa(`preview-${index + 1}`)}`,
        ]),
      ),
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow.locator('.project-card')).toHaveCount(40);
    await expect
      .poll(async () =>
        page.evaluate(() =>
          ((window as any).__PROJECTS_CALLS__ as Array<{ cmd: string }>).filter(
            (entry) => entry.cmd === 'get_thread_preview',
          ).length,
        ),
      )
      .toBeGreaterThan(0);

    const calls = await page.evaluate(() => (window as any).__PROJECTS_CALLS__ as Array<{ cmd: string }>);
    const previewCalls = calls.filter((entry) => entry.cmd === 'get_thread_preview');
    expect(previewCalls.length).toBeLessThan(40);
    expect(calls.filter((entry) => entry.cmd === 'get_thread_latest_version')).toHaveLength(0);
  });

  test('Given latest version lacks thumbnail When older version has preview Then project card reports no current preview', async ({ page }) => {
    const previewImage = `data:image/png;base64,${btoa('older-preview')}`;
    await installProjectSwitcherMocks({
      history: [
        {
          id: 'thread-preview',
          title: 'Paged Preview Thread',
          summary: '',
          updatedAt: Date.UTC(2026, 5, 1),
          messages: [],
          genieTraits: null,
          versionCount: 2,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'ready',
          finalizedAt: null,
          pendingConfirm: null,
        },
      ],
      threadPreviews: {
        'thread-preview': null,
      },
      messagePages: {
        'thread-preview': [
          {
            id: 'older-version',
            role: 'assistant',
            status: 'success',
            imageData: previewImage,
            timestamp: 100,
            content: '',
          },
          {
            id: 'current-head',
            role: 'assistant',
            status: 'error',
            imageData: null,
            timestamp: 200,
            content: '',
          },
        ],
      },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const card = page.locator('[data-window-id="projects"] .project-card').filter({ hasText: 'Paged Preview Thread' });
    const preview = card.locator('.preview-frame');
    await expect(preview).toHaveAttribute('data-preview-state', 'empty');
    await expect(preview.locator('img')).toHaveCount(0);
    await expect(card.getByText('NO PREVIEW')).toBeVisible();
  });

  test('Given a project starts without preview When preview event arrives Then card replaces NO PREVIEW', async ({ page }) => {
    const previewImage = `data:image/png;base64,${btoa('fresh-preview')}`;
    await installProjectSwitcherMocks({
      history: [
        {
          id: 'thread-preview',
          title: 'Event Preview Thread',
          summary: '',
          updatedAt: Date.UTC(2026, 5, 2),
          messages: [],
          genieTraits: null,
          versionCount: 1,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'ready',
          finalizedAt: null,
          pendingConfirm: null,
        },
      ],
      latestVersions: {
        'thread-preview': {
          id: 'version-1',
          role: 'assistant',
          status: 'success',
          artifactBundle: { modelStlPath: '/mock/version-1.stl' },
          imageData: null,
        },
      },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const card = page.locator('[data-window-id="projects"] .project-card').filter({ hasText: 'Event Preview Thread' });
    await expect(card.getByText('NO PREVIEW')).toBeVisible();

    await page.evaluate(({ previewImage }) => {
      window.dispatchEvent(
        new CustomEvent('ecky:version-preview-updated', {
          detail: {
            threadId: 'thread-preview',
            messageId: 'version-1',
            imageData: previewImage,
          },
        }),
      );
    }, { previewImage });

    const preview = card.locator('.preview-frame');
    await expect(preview).toHaveAttribute('data-preview-state', 'ready');
    await expect(preview.locator('img')).toHaveAttribute('src', previewImage);
    await expect(card.getByText('NO PREVIEW')).toHaveCount(0);
  });

  test('Given an active project When it is completed Then the same project and full history appear in Completed', async ({ page }) => {
    await installProjectSwitcherMocks({
      history: [
        {
          id: 'project-same-id',
          title: 'Bike light enclosure',
          summary: 'Three successful iterations',
          updatedAt: Date.UTC(2026, 6, 24),
          messages: [],
          genieTraits: null,
          versionCount: 3,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          finalizedAt: null,
          pendingConfirm: null,
        },
      ],
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    const activeCard = projectsWindow.locator('.project-card').filter({ hasText: 'Bike light enclosure' });
    await activeCard.getByRole('button', { name: 'COMPLETE' }).click();
    await projectsWindow.getByRole('button', { name: 'COMPLETED' }).click();

    const completedCard = projectsWindow.locator('.project-card').filter({ hasText: 'Bike light enclosure' });
    await expect(completedCard).toHaveAttribute('data-project-id', 'project-same-id');
    await expect(completedCard).toContainText('3 versions');

    const calls = await page.evaluate(() => (window as any).__PROJECTS_CALLS__);
    expect(calls).toContainEqual({
      cmd: 'finalize_thread_intent',
      args: {
        input: {
          threadId: 'project-same-id',
          selectedMessageId: null,
        },
      },
    });
  });

  test('Given a deleted project When it is recovered Then the same project ID returns to Active', async ({ page }) => {
    await installProjectSwitcherMocks({
      deletedProjectPages: {
        first: {
          items: [
            {
              id: 'deleted-project-id',
              title: 'Recovered enclosure',
              summary: 'Deleted by mistake',
              updatedAt: Date.UTC(2026, 6, 23),
              deletedAt: Date.UTC(2026, 6, 24),
              versionCount: 4,
            },
          ],
          nextBefore: null,
          hasMore: false,
        },
      },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await projectsWindow.getByRole('button', { name: 'TRASH' }).click();
    const deletedCard = projectsWindow.locator('.project-card').filter({ hasText: 'Recovered enclosure' });
    await expect(deletedCard).toHaveAttribute('data-project-id', 'deleted-project-id');
    await deletedCard.getByRole('button', { name: 'RECOVER' }).click();

    await projectsWindow.getByRole('button', { name: 'ACTIVE' }).click();
    const restoredCard = projectsWindow.locator('.project-card').filter({ hasText: 'Recovered enclosure' });
    await expect(restoredCard).toHaveAttribute('data-project-id', 'deleted-project-id');

    const calls = await page.evaluate(() => (window as any).__PROJECTS_CALLS__);
    expect(calls).toContainEqual({
      cmd: 'restore_deleted_thread',
      args: { id: 'deleted-project-id' },
    });
  });

  test('Given more deleted projects exist When Trash opens Then it loads 24 and requests the next cursor on demand', async ({ page }) => {
    const firstItems = Array.from({ length: 24 }, (_, index) => ({
      id: `deleted-${index + 1}`,
      title: `Deleted project ${index + 1}`,
      summary: '',
      updatedAt: Date.UTC(2026, 6, 1),
      deletedAt: Date.UTC(2026, 6, 24),
      versionCount: 1,
    }));
    await installProjectSwitcherMocks({
      deletedProjectPages: {
        first: {
          items: firstItems,
          nextBefore: 'cursor-24',
          hasMore: true,
        },
        'cursor-24': {
          items: [
            {
              id: 'deleted-25',
              title: 'Deleted project 25',
              summary: '',
              updatedAt: Date.UTC(2026, 6, 1),
              deletedAt: Date.UTC(2026, 6, 23),
              versionCount: 2,
            },
          ],
          nextBefore: null,
          hasMore: false,
        },
      },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await projectsWindow.getByRole('button', { name: 'TRASH' }).click();
    await expect(projectsWindow.locator('.project-card')).toHaveCount(24);
    await expect(projectsWindow.getByText('Deleted project 25', { exact: true })).toHaveCount(0);

    await projectsWindow.getByRole('button', { name: 'LOAD MORE' }).click();
    await expect(projectsWindow.locator('.project-card')).toHaveCount(25);
    await expect(projectsWindow.getByText('Deleted project 25', { exact: true })).toBeVisible();

    const pageCalls = await page.evaluate(() =>
      ((window as any).__PROJECTS_CALLS__ as Array<{ cmd: string; args?: Record<string, unknown> }>)
        .filter((entry) => entry.cmd === 'get_deleted_threads_page'),
    );
    expect(pageCalls).toEqual([
      { cmd: 'get_deleted_threads_page', args: { before: null, limit: 24 } },
      { cmd: 'get_deleted_threads_page', args: { before: 'cursor-24', limit: 24 } },
    ]);
  });

  test('Given project navigation When reusable assets are needed Then Library is a separate window', async ({ page }) => {
    await installProjectSwitcherMocks()({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow.getByRole('button', { name: 'PACKAGES' })).toHaveCount(0);
    await expect(projectsWindow.getByRole('button', { name: 'ACTIVE' })).toBeVisible();
    await expect(projectsWindow.getByRole('button', { name: 'COMPLETED' })).toBeVisible();

    await page.getByRole('button', { name: 'LIBRARY' }).click();
    const libraryWindow = page.locator('[data-window-id="library"]');
    await expect(libraryWindow).toBeVisible();
    await expect(libraryWindow.getByRole('button', { name: 'COMPONENT PACKAGES' })).toBeVisible();
    await expect(libraryWindow.getByRole('button', { name: 'FREECAD PARTS' })).toBeVisible();
    await expect(libraryWindow.getByRole('button', { name: 'CATALOG' })).toHaveCount(0);
  });

  test('Given Projects is open When viewport narrows Then window and compact card remain fully reachable', async ({ page }) => {
    await installProjectSwitcherMocks({
      history: [
        {
          id: 'responsive-project',
          title: 'Responsive enclosure',
          summary: 'Compact card proof',
          updatedAt: Date.UTC(2026, 6, 25),
          messages: [],
          genieTraits: null,
          versionCount: 2,
          pendingCount: 0,
          queuedCount: 0,
          errorCount: 0,
          status: 'active',
          finalizedAt: null,
          pendingConfirm: null,
        },
      ],
    })({ page });

    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.setViewportSize({ width: 320, height: 700 });

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect.poll(async () =>
      projectsWindow.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return {
          left: Math.round(rect.left),
          top: Math.round(rect.top),
          right: Math.round(rect.right),
          bottom: Math.round(rect.bottom),
        };
      }),
    ).toEqual({ left: 0, top: 80, right: 320, bottom: 580 });

    const card = projectsWindow.locator('.project-card').filter({ hasText: 'Responsive enclosure' });
    await expect(card).toBeVisible();
    await expect(card.getByRole('button', { name: 'COMPLETE' })).toBeVisible();
  });

  test.describe('Completed projects', () => {
    test.beforeEach(
      installProjectSwitcherMocks({
        inventory: [
          {
            id: 'completed-1',
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
        threadPreviews: {
          'completed-1': `data:image/png;base64,${btoa('completed-preview')}`,
        },
        threadPreviewDelayMs: 3000,
      }),
    );

    test('Given slow preview payload, when Completed opens, then summary cards render without full-version fetches', async ({ page }) => {
      await page.goto('/');
      await page.getByRole('button', { name: 'PROJECTS' }).click();
      await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'COMPLETED' }).click();

      const projectsWindow = page.locator('[data-window-id="projects"]');
      await expect(projectsWindow.locator('.project-card')).toHaveCount(1);
      await expect(projectsWindow.getByText('Tradescantia zebrina pot')).toBeVisible();
      await expect(projectsWindow.getByText('LOADING COMPLETED...')).toHaveCount(0);

      const calls = await page.evaluate(() => (window as any).__PROJECTS_CALLS__ as Array<{ cmd: string }>);
      expect(calls.filter((entry) => entry.cmd === 'get_thread_latest_version')).toHaveLength(0);
      expect(calls.filter((entry) => entry.cmd === 'get_thread_messages_page')).toHaveLength(0);
    });
  });

  test('Given completed-project loading fails, when Completed opens, then raw error shows', async ({ page }) => {
    await installProjectSwitcherMocks({
      inventoryError: {
        message: 'Inventory query failed',
        details: 'database is locked',
      },
    })({ page });

    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await page.locator('[data-window-id="projects"]').getByRole('button', { name: 'COMPLETED' }).click();

    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow.getByText('COMPLETED LOAD ERROR')).toBeVisible();
    await expect(projectsWindow.getByText('Inventory query failed')).toBeVisible();
    await expect(projectsWindow.getByText('database is locked')).toBeVisible();
  });
});
