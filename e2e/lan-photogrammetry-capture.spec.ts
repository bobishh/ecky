import { expect, test, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';

type MockConfig = Record<string, unknown>;
const captureClientHtml = readFileSync(new URL('../src-tauri/assets/capture_client.html', import.meta.url), 'utf8');
const captureMetricsSource = readFileSync(new URL('../src-tauri/assets/capture_metrics.mjs', import.meta.url), 'utf8');

async function installPhoneCaptureRoute(page: Page, cameraError?: string, failFirstUpload = false) {
  const frames: Array<Record<string, unknown>> = [];
  let networkAvailable = !failFirstUpload;
  let sessionState = 'capturing';
  let reconstructionPolls = 0;
  await page.addInitScript(({ cameraError, fastAssessment }) => {
    (window as any).__CAPTURE_WAKE__ = { requests: 0, releases: 0 };
    Object.defineProperty(navigator, 'wakeLock', {
      configurable: true,
      value: {
        request: async () => {
          (window as any).__CAPTURE_WAKE__.requests += 1;
          return {
            released: false,
            release: async () => { (window as any).__CAPTURE_WAKE__.releases += 1; },
            addEventListener() {},
          };
        },
      },
    });
    const realSetInterval = window.setInterval.bind(window);
    window.setInterval = ((handler: TimerHandler, timeout?: number, ...args: unknown[]) =>
      realSetInterval(handler, fastAssessment && timeout === 500 ? 20 : timeout, ...args)) as typeof window.setInterval;
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: {
        getUserMedia: async () => {
          if (cameraError) throw new DOMException(cameraError, 'NotAllowedError');
          return new MediaStream();
        },
      },
    });
    window.createImageBitmap = async () => ({ width: 3024, height: 4032, close() {} }) as ImageBitmap;
    Object.defineProperty(HTMLVideoElement.prototype, 'videoWidth', { configurable: true, get: () => 640 });
    Object.defineProperty(HTMLVideoElement.prototype, 'videoHeight', { configurable: true, get: () => 480 });
    HTMLMediaElement.prototype.play = async () => {};
    let assessmentCount = 0;
    Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', { configurable: true, value: function () {
      return {
        drawImage() {},
        getImageData: () => {
          const data = new Uint8ClampedArray(160 * 120 * 4);
          const view = Math.floor(assessmentCount / 6) % 3;
          assessmentCount += 1;
          const left = 35 + view * 20;
          for (let y = 0; y < 120; y += 1) {
            for (let x = 0; x < 160; x += 1) {
              const index = (y * 160 + x) * 4;
              const object = x >= left && x < left + 70 && y >= 25 && y < 95;
              const value = object ? ((x + y) % 2 === 0 ? 70 : 180) : 40;
              data[index] = value; data[index + 1] = value; data[index + 2] = value; data[index + 3] = 255;
            }
          }
          return { data };
        },
      };
    } });
    let blobCounter = 0;
    HTMLCanvasElement.prototype.toBlob = function (callback) {
      blobCounter += 1;
      callback(new Blob([new Uint8Array([0xff, 0xd8, blobCounter, 0xd9])], { type: 'image/jpeg' }));
    };
    let now = 1_000_000;
    Date.now = () => (now += 2_000);
  }, { cameraError, fastAssessment: !failFirstUpload });
  await page.route('https://capture.test/**', async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/capture/abc123') {
      await route.fulfill({ status: 200, contentType: 'text/html', body: captureClientHtml });
      return;
    }
    if (url.pathname === '/capture-assets/capture_metrics.mjs') {
      await route.fulfill({ status: 200, contentType: 'text/javascript', body: captureMetricsSource });
      return;
    }
    if (url.pathname.endsWith('/frames') && route.request().method() === 'GET') {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ sessionId: 'abc123', frames }) });
      return;
    }
    if (url.pathname.includes('/frames/') && route.request().method() === 'POST') {
      if (!networkAvailable) {
        await route.fulfill({ status: 503, contentType: 'text/plain', body: 'LAN disconnected by test' });
        return;
      }
      frames.push({ frameId: url.pathname.split('/').pop(), contentDigest: route.request().headers()['x-content-digest'] });
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({
        created: true, frame: { serverAssessment: { coveragePercent: Math.min(100, frames.length * 4), guidance: 'Continue around object' } },
      }) });
      return;
    }
    if (url.pathname.endsWith('/finish') && route.request().method() === 'POST') {
      sessionState = 'reconstructing';
      reconstructionPolls = 0;
    } else if (route.request().method() === 'GET' && sessionState === 'reconstructing') {
      reconstructionPolls += 1;
      if (reconstructionPolls >= 2) sessionState = 'preview';
    }
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ sessionId: 'abc123', state: sessionState }) });
  });
  return {
    frames,
    restoreNetwork: () => { networkAvailable = true; },
    requestMorePhotos: () => { sessionState = 'capturing'; },
  };
}

async function installCaptureShellMocks(page: Page, config: MockConfig = {}) {
  await page.addInitScript(({ config }) => {
    const mockWindow = window as any;
    mockWindow.__CAPTURE_CALLS__ = [];
    let externalShapeSources = structuredClone((config.externalShapeSources ?? []) as Array<Record<string, any>>);
    let preparedCaptureStlPath = '/tmp/capture-model.stl';
    window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
    (window.__TAURI_INTERNALS__ as any).convertFileSrc = (path: string) => path;
    window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: Record<string, unknown>) => {
      mockWindow.__CAPTURE_CALLS__.push({ cmd, args });
      if (cmd === 'get_config') {
        return {
          engines: [],
          selectedEngineId: '',
          freecadCmd: '',
          assets: [],
          microwave: { humId: null, dingId: null, muted: true },
          voice: { sttLanguageCode: 'en-US' },
          mcp: { port: null, maxSessions: null, mode: 'active', primaryAgentId: null, promptTimeoutSecs: 1800, eckyAstAuthoring: false, autoAgents: [] },
          hasSeenOnboarding: true,
          connectionType: 'mcp',
          defaultEngineKind: 'ecky',
          defaultSourceLanguage: 'ecky',
          defaultGeometryBackend: 'mesh',
          maxGenerationAttempts: 1,
          maxVerifyAttempts: 0,
          ...config,
        };
      }
      if (cmd === 'save_config') return null;
      if (cmd === 'get_runtime_capabilities') {
        return {
          freecad: { available: true, detail: 'Ready', path: '/mock/freecadcmd' },
          build123d: { available: true, detail: 'Ready', path: '/mock/python3' },
          mesh: { available: true, detail: 'bundled', path: null },
          recommendedAuthoringContext: { engineKind: 'ecky', sourceLanguage: 'ecky', geometryBackend: 'mesh' },
        };
      }
      if (cmd === 'get_history') {
        if (!config.captureHistoryRun) return [];
        return [{
          id: 'capture-thread', title: 'Finger fixture', summary: '', updatedAt: 50,
          messages: [], genieTraits: null, versionCount: 0, pendingCount: 0,
          queuedCount: 0, errorCount: 0, status: 'active', finalizedAt: null, pendingConfirm: null,
        }];
      }
      if (cmd === 'get_last_design') return null;
      if (cmd === 'list_external_shape_sources') return structuredClone(externalShapeSources);
      if (cmd === 'apply_external_shape_edit') {
        const input = (args?.input ?? {}) as Record<string, any>;
        const edit = (input.edit ?? {}) as Record<string, any>;
        const cropNodeId = Number(edit.cropNodeId ?? -1);
        externalShapeSources = externalShapeSources.map(source => ({
          ...source,
          sourceDigest: 'sha256:source-after-remove',
          planeCrops: (source.planeCrops ?? []).filter((crop: Record<string, unknown>) => crop.nodeId !== cropNodeId),
        }));
        return {
          version: {
            threadId: input.threadId,
            baseMessageId: input.baseMessageId ?? null,
            messageId: 'external-plane-remove-version',
            status: 'success',
            designOutput: {
              title: 'Finger fixture', versionName: 'Plane crop removed', response: 'Plane crop removed.',
              interactionMode: 'design', macroCode: '(model (part head_only (solidify (import-stl "/tmp/rocksteady.stl"))))',
              macroDialect: 'ecky', sourceLanguage: 'ecky', geometryBackend: 'mesh', engineKind: 'ecky',
              uiSpec: { fields: [] }, initialParams: {}, postProcessing: null,
            },
            artifactBundle: {
              modelId: 'external-plane-remove-model', sourceKind: 'generated', sourceLanguage: 'ecky',
              geometryBackend: 'mesh', engineKind: 'ecky', contentHash: 'sha256:source-after-remove', artifactVersion: 1,
              fcstdPath: null, manifestPath: '/tmp/external-plane-remove.json',
              modelStlPath: '/Users/bogdan/Downloads/rocksteady-1.stl', viewerAssets: [], exportArtifacts: [],
              edgeTargets: [], faceTargets: [], calloutAnchors: [], measurementGuides: [],
            },
            modelManifest: {
              schemaVersion: 1, modelId: 'external-plane-remove-model', sourceKind: 'generated', sourceLanguage: 'ecky',
              geometryBackend: 'mesh', engineKind: 'ecky', sourceDigest: 'sha256:source-after-remove',
              document: { documentName: 'Finger fixture', documentLabel: 'Finger fixture', objectCount: 1, warnings: [] },
              parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [], previewViews: [],
              advisories: [], selectionTargets: [], measurementAnnotations: [], taggedAnchors: {}, analysisDeclarations: [], warnings: [],
              enrichmentState: { status: 'none', proposals: [] },
            },
            snapshotId: 'snapshot-external-plane-remove',
            parserMatched: true,
            error: null,
          },
          sourceDigest: 'sha256:source-after-remove',
          externalSources: structuredClone(externalShapeSources),
        };
      }
      if (cmd === 'get_default_macro') return '(solid blank)';
      if (cmd === 'get_active_agent_sessions') return [];
      if (cmd === 'get_agent_terminal_snapshots') return [];
      if (cmd === 'get_thread_agent_state') {
        return {
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
      if (cmd === 'start_capture_session') {
        if (typeof config.captureStartError === 'string') throw new Error(config.captureStartError);
        const target = (args?.target ?? null) as Record<string, unknown> | null;
        const targetThreadId = String(target?.threadId ?? 'rust-capture-thread');
        return {
          sessionId: 'abc123',
          targetThreadId,
          targetMessageId: target?.messageId ?? null,
          targetTitle: target ? 'Finger fixture' : `Capture ${targetThreadId.slice(0, 8)}`,
          targetSource: String(target?.source ?? ''),
          targetSourceLanguage: String(target?.sourceLanguage ?? 'ecky'),
          startedFromEmpty: target === null,
          pairingToken: 'abc123',
          pairingUrl: 'https://192.0.2.10:44000/capture/abc123',
          trustUrl: 'http://192.0.2.10:44001/trust',
          protocolVersion: 1,
          clientCapabilities: { metricDepth: false, cameraIntrinsics: false, cameraPose: false, depthSidecars: false },
          state: 'pairing',
          createdAt: 1,
          expiresAt: 999999,
          acceptedFrameCount: 0,
        };
      }
      const durableCaptureRun = {
        id: 'durable-capture-1', targetThreadId: 'capture-thread', targetMessageId: null,
        title: 'Finger fixture capture', state: 'preview', createdAt: 10, updatedAt: 50,
        acceptedFrameCount: 48,
        meshPreview: { stlPath: '/tmp/capture-model.stl', triangleCount: 1234,
          boundsMm: [42, 31, 18], scaleLabel: 'Restored capture coordinates', warnings: ['Verify dimensions'] },
        derivedStlPath: null, cropBounds: null, previewScale: 0.05,
        targetSource: '', targetSourceLanguage: 'ecky', startedFromEmpty: false, rawError: null,
      };
      const reopenedCaptureSession = {
        sessionId: 'durable-capture-1', targetThreadId: 'capture-thread', targetMessageId: null,
        pairingToken: 'rotated-token', pairingUrl: 'https://192.0.2.10:44000/capture/rotated-token',
        trustUrl: 'http://192.0.2.10:44001/trust', protocolVersion: 1, clientCapabilities: {},
        state: 'preview', createdAt: 10, expiresAt: 999999, acceptedFrameCount: 48,
        reconstructionProgress: 1, meshPreview: durableCaptureRun.meshPreview,
      };
      if (cmd === 'list_capture_runs') return config.captureHistoryRun ? [durableCaptureRun] : [];
      if (cmd === 'reopen_capture_run') return { run: durableCaptureRun, session: reopenedCaptureSession };
      if (cmd === 'adopt_latest_capture_run') return {
        run: { ...durableCaptureRun, targetThreadId: String((args?.target as any)?.threadId ?? 'rust-capture-thread'),
          title: (args?.target as any)?.threadId ? 'Finger fixture' : 'Capture rust-cap',
          targetSource: String((args?.target as any)?.source ?? ''), startedFromEmpty: args?.target == null },
        session: { ...reopenedCaptureSession, targetThreadId: String((args?.target as any)?.threadId ?? 'rust-capture-thread') },
      };
      if (cmd === 'save_capture_preview_settings') return null;
      if (cmd === 'get_thread_latest_version') return null;
      if (cmd === 'get_thread_messages_page') return { messages: [], nextBefore: null, hasMore: false };
      if (cmd === 'get_capture_session_status') {
        if (config.captureStatus === 'preview') {
          return {
            sessionId: 'abc123', pairingToken: 'abc123', pairingUrl: '', trustUrl: '',
            protocolVersion: 1, clientCapabilities: {}, state: 'preview', createdAt: 1,
            expiresAt: 999999, acceptedFrameCount: 48, reconstructionProgress: 1,
            meshPreview: { stlPath: '/tmp/capture-model.stl', triangleCount: 1234,
              boundsMm: [42, 31, 18], scaleLabel: 'meters converted to millimeters', warnings: ['Verify dimensions'] },
          };
        }
        if (config.captureStatus === 'failed') {
          return {
            sessionId: 'abc123', pairingToken: 'abc123', pairingUrl: '', trustUrl: '',
            protocolVersion: 1, clientCapabilities: {}, state: 'failed', createdAt: 1,
            expiresAt: 999999, acceptedFrameCount: 12, rawError: 'Object Capture request failed by test',
          };
        }
        return {
          sessionId: 'abc123', pairingToken: 'abc123',
          pairingUrl: 'https://192.0.2.10:44000/capture/abc123',
          trustUrl: 'http://192.0.2.10:44001/trust',
          protocolVersion: 1,
          clientCapabilities: { metricDepth: false, cameraIntrinsics: false, cameraPose: false, depthSidecars: false },
          state: 'pairing', createdAt: 1, expiresAt: 999999, acceptedFrameCount: 0,
        };
      }
      if (cmd === 'cancel_capture_session') {
        return {
          sessionId: 'abc123',
          pairingToken: 'abc123',
          pairingUrl: 'https://192.0.2.10:44000/capture/abc123',
          trustUrl: 'http://192.0.2.10:44001/trust',
          protocolVersion: 1,
          clientCapabilities: { metricDepth: false, cameraIntrinsics: false, cameraPose: false, depthSidecars: false },
          state: 'cancelled',
          createdAt: 1,
          expiresAt: 999999,
          acceptedFrameCount: 0,
        };
      }
      if (cmd === 'resume_capture_session') return {
        sessionId: 'abc123', targetThreadId: String(args?.threadId ?? 'capture-thread'), targetMessageId: null,
        pairingToken: 'abc123', pairingUrl: 'https://192.0.2.10:44000/capture/abc123',
        trustUrl: 'http://192.0.2.10:44001/trust', protocolVersion: 1, clientCapabilities: {},
        state: 'capturing', createdAt: 1, expiresAt: 999999, acceptedFrameCount: 48,
      };
      if (cmd === 'import_freecad_library_part') return {
        modelId: 'capture-mesh', sourceKind: 'importedMesh', contentHash: 'capture', artifactVersion: 1,
        fcstdPath: '', manifestPath: '/tmp/capture-manifest.json', modelStlPath: '/tmp/capture-model.stl',
        viewerAssets: [], exportArtifacts: [], geometryBackend: 'mesh', sourceLanguage: 'ecky', engineKind: 'ecky',
      };
      if (cmd === 'get_model_manifest') return {
        schemaVersion: 1, modelId: 'capture-mesh', sourceKind: 'importedMesh', engineKind: 'ecky',
        sourceLanguage: 'ecky', geometryBackend: 'mesh',
        document: { documentName: 'Capture', documentLabel: 'Capture', sourcePath: '/tmp/capture-model.stl', objectCount: 1, warnings: [] },
        parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [],
        advisories: [], selectionTargets: [], measurementAnnotations: [], warnings: [],
        enrichmentState: { status: 'none', proposals: [] },
      };
      if (cmd === 'render_model') return {
        modelId: 'capture-solidified', sourceKind: 'generated', contentHash: 'capture-solidified', artifactVersion: 1,
        fcstdPath: '', manifestPath: '/tmp/capture-manifest.json', modelStlPath: '/tmp/capture-model.stl',
        viewerAssets: [], exportArtifacts: [], geometryBackend: 'mesh', sourceLanguage: 'ecky', engineKind: 'ecky',
      };
      if (cmd === 'apply_manual_code') {
        const input = args?.input as Record<string, any>;
        return {
          threadId: input.threadId, baseMessageId: input.baseMessageId ?? null,
          messageId: input.persist ? 'capture-version' : null, status: 'success',
          designOutput: {
            title: input.title ?? 'Capture', versionName: input.versionName ?? 'Capture', response: 'Capture committed.',
            interactionMode: 'design', macroCode: input.source, macroDialect: 'ecky', engineKind: 'ecky',
            sourceLanguage: 'ecky', geometryBackend: 'mesh', uiSpec: input.uiSpec, initialParams: input.parameters,
            postProcessing: input.postProcessing ?? null,
          },
          artifactBundle: {
            modelId: 'capture-solidified', sourceKind: 'generated', contentHash: 'capture-solidified', artifactVersion: 1,
            fcstdPath: '', manifestPath: '/tmp/capture-manifest.json', modelStlPath: '/tmp/capture-model.stl',
            viewerAssets: [], exportArtifacts: [], geometryBackend: 'mesh', sourceLanguage: 'ecky', engineKind: 'ecky',
          },
          modelManifest: {
            schemaVersion: 1, modelId: 'capture-solidified', sourceKind: 'generated', engineKind: 'ecky',
            sourceLanguage: 'ecky', geometryBackend: 'mesh',
            document: { documentName: 'Capture', documentLabel: 'Capture', objectCount: 1, warnings: [] },
            parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [], advisories: [],
            selectionTargets: [], measurementAnnotations: [], warnings: [], enrichmentState: { status: 'none', proposals: [] },
          },
          snapshotId: 'capture-commit-snapshot', parserMatched: true, error: null,
        };
      }
      if (cmd === 'prepare_capture_preview') {
        if (args?.cropBounds && config.cropError) throw new Error(String(config.cropError));
        preparedCaptureStlPath = args?.cropBounds ? '/tmp/capture-box-crop.stl' : '/tmp/capture-model.stl';
        return { artifactBundle: {
          modelId: 'capture-mesh', sourceKind: 'generated', contentHash: 'capture', artifactVersion: 1,
          fcstdPath: '', manifestPath: '/tmp/capture-manifest.json',
          modelStlPath: args?.cropBounds ? '/tmp/capture-box-crop.stl' : '/tmp/capture-model.stl',
          viewerAssets: [], exportArtifacts: [], geometryBackend: 'mesh', sourceLanguage: 'ecky', engineKind: 'ecky',
        },
        modelManifest: {
          schemaVersion: 1, modelId: 'capture-mesh', sourceKind: 'generated', engineKind: 'ecky',
          sourceLanguage: 'ecky', geometryBackend: 'mesh',
          document: { documentName: 'Capture', documentLabel: 'Capture', sourcePath: '/tmp/capture-model.stl', objectCount: 1, warnings: [] },
          parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [],
          advisories: [], selectionTargets: [], measurementAnnotations: [], warnings: [],
          enrichmentState: { status: 'none', proposals: [] },
        } };
      }
      if (cmd === 'apply_capture_preview') {
        if (config.captureApplyError) throw new Error(String(config.captureApplyError));
        if (config.captureApplyPending) {
          await new Promise<void>((resolve) => {
            (window as any).__RESOLVE_CAPTURE_APPLY__ = resolve;
          });
        }
        const source = `(model (params (number capture_scale_abc123 0.05)) (part capture_abc123 (scale capture_scale_abc123 capture_scale_abc123 capture_scale_abc123 (solidify (import-stl "${preparedCaptureStlPath}")))))`;
        return {
          source,
          draft: {
            threadId: 'rust-capture-thread', baseMessageId: null, messageId: null, status: 'success',
            designOutput: {
              title: 'Capture rust-cap', versionName: 'Capture Draft', response: 'Code draft applied.',
              interactionMode: 'design', macroCode: source, macroDialect: 'ecky', engineKind: 'ecky',
              sourceLanguage: 'ecky', geometryBackend: 'mesh', uiSpec: { fields: [] }, initialParams: {}, postProcessing: null,
            },
            artifactBundle: {
              modelId: 'capture-solidified', sourceKind: 'generated', contentHash: 'capture-solidified', artifactVersion: 1,
              fcstdPath: '', manifestPath: '/tmp/capture-manifest.json', modelStlPath: '/tmp/capture-model.stl',
              viewerAssets: [], exportArtifacts: [], geometryBackend: 'mesh', sourceLanguage: 'ecky', engineKind: 'ecky',
            },
            modelManifest: {
              schemaVersion: 1, modelId: 'capture-solidified', sourceKind: 'generated', engineKind: 'ecky',
              sourceLanguage: 'ecky', geometryBackend: 'mesh',
              document: { documentName: 'Capture', documentLabel: 'Capture', objectCount: 1, warnings: [] },
              parts: [], parameterGroups: [], controlPrimitives: [], controlRelations: [], controlViews: [], advisories: [],
              selectionTargets: [], measurementAnnotations: [], warnings: [], enrichmentState: { status: 'none', proposals: [] },
            },
            snapshotId: 'capture-apply-snapshot', parserMatched: true, error: null,
          },
        };
      }
      if (cmd === 'retry_capture_reconstruction') return {
        sessionId: 'abc123', pairingToken: 'abc123', pairingUrl: '', trustUrl: '', protocolVersion: 1,
        clientCapabilities: {}, state: 'reconstructing', createdAt: 1, expiresAt: 999999,
        acceptedFrameCount: 20, reconstructionProgress: 0,
      };
      if (cmd === 'save_model_manifest' || cmd === 'save_last_design') return null;
      return null;
    };
  }, { config });
}

async function routeRestoredCaptureStl(page: Page, urlPattern = '**/tmp/capture-model.stl*') {
  const previewStl = Buffer.alloc(84 + 50);
  previewStl.writeUInt32LE(1, 80);
  [[0, 0, 0], [40, 0, 0], [0, 30, 10]].forEach((vertex, vertexIndex) => {
    vertex.forEach((value, axis) => previewStl.writeFloatLE(value, 84 + 12 + vertexIndex * 12 + axis * 4));
  });
  await page.route(urlPattern, route => route.fulfill({
    status: 200,
    contentType: 'model/stl',
    body: previewStl,
  }));
}

test('Given External Shapes opens When Capture is active Then pairing stays scoped and trust setup is on demand', async ({ page }) => {
  await installCaptureShellMocks(page);
  await page.setViewportSize({ width: 1440, height: 960 });

  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);

  await page.getByRole('button', { name: 'Work with external shapes', exact: true }).click();

  await expect(page.locator('[data-window-id="capture"]')).toBeVisible();
  await expect(page.locator('[data-window-id="capture"]')).toContainText('External Shapes');
  const externalWorkflow = page.getByRole('tablist', { name: 'External shapes workflow' });
  await expect(externalWorkflow).toBeVisible();
  await expect(externalWorkflow.getByRole('tab')).toHaveCount(3);
  await expect(externalWorkflow.getByRole('tab', { name: 'GUIDES' })).toHaveCount(0);
  await expect(externalWorkflow.getByRole('tab', { name: 'RECONSTRUCT' })).toHaveCount(0);
  await expect(externalWorkflow.getByRole('tab', { name: 'VALIDATE' })).toHaveCount(0);
  await expect(page.getByRole('tab', { name: 'CAPTURE', exact: true })).toHaveAttribute('aria-selected', 'true');
  const captureWorkflow = page.getByRole('tablist', { name: 'Capture workflow' });
  await expect(captureWorkflow.getByRole('tab', { name: 'SCAN' })).toHaveAttribute('aria-selected', 'true');
  await expect(captureWorkflow.getByRole('tab', { name: 'GUIDED BREP' })).toBeVisible();
  await expect(page.getByText('PAIR PHONE')).toBeVisible();
  await expect(page.getByText('Camera permission pending')).toBeVisible();
  const urlText = page.locator('.capture-panel__pairing-url');
  await expect(urlText).toContainText('No pairing session yet');
  await expect(page.getByRole('button', { name: 'START CAPTURE' })).toBeVisible();

  await page.getByRole('button', { name: 'START CAPTURE' }).click();
  const startIntent = await page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .find((call: { cmd: string }) => call.cmd === 'start_capture_session'));
  expect(startIntent.args).toEqual({ target: null });
  await expect(page.getByText('OPEN LINK ON PHONE')).toBeVisible();
  await expect(page.getByText('Waiting for phone camera')).toBeVisible();
  await expect(urlText).toHaveText('https://192.0.2.10:44000/capture/abc123');
  await expect(page.getByRole('link', { name: 'INSTALL PHONE CERTIFICATE' })).toBeHidden();
  await expect(page.getByText('Settings > General > About > Certificate Trust Settings')).toBeHidden();
  await page.getByText('PHONE TRUST SETUP').click();
  await expect(page.getByRole('link', { name: 'INSTALL PHONE CERTIFICATE' })).toHaveAttribute('href', 'http://192.0.2.10:44001/trust');
  await expect(page.getByText('2. ENABLE FULL TRUST')).toBeVisible();
  await expect(page.getByText('Settings > General > About > Certificate Trust Settings')).toBeVisible();
  await expect(page.locator('.capture-panel__qr')).toHaveCount(2);

  await page.getByRole('tab', { name: 'CROP', exact: true }).click();
  await expect(page.getByText('PAIR PHONE')).toHaveCount(0);
  await expect(page.getByText('SELECT OR CAPTURE A SOURCE SHAPE')).toBeVisible();
  await page.getByRole('tab', { name: 'CAPTURE', exact: true }).click();

  await page.getByRole('button', { name: 'CANCEL' }).click();
  await expect(page.getByText('PAIR PHONE')).toBeVisible();
  await expect(page.getByText('Session cancelled')).toBeVisible();
});

test('Given Rocksteady source contains one import STL When Import opens Then raw mesh is selected for Crop', async ({ page }) => {
  await installCaptureShellMocks(page, {
    captureHistoryRun: true,
    externalShapeSources: [{
      nodeId: 16,
      partKey: 'head_only',
      path: '/Users/bogdan/Downloads/rocksteady-1.stl',
      displayName: 'rocksteady-1.stl',
      sourceDigest: 'sha256:source',
      contentDigest: 'sha256:mesh',
      byteLength: 2221684,
      exists: true,
      planeCrops: [{
        nodeId: 19,
        origin: [37.22, 38.84, 91.19],
        normal: [0.42, 0.41, -0.81],
        keepPositive: true,
      }],
    }],
  });
  await routeRestoredCaptureStl(page, '**/*rocksteady-1.stl*');
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  const project = page.locator('[data-window-id="projects"] .project-card').filter({ hasText: 'Finger fixture' });
  await project.getByRole('button', { name: 'OPEN' }).click();

  await page.getByRole('button', { name: 'Work with external shapes', exact: true }).click();
  await page.getByRole('tab', { name: 'IMPORT', exact: true }).click();

  await expect(page.getByRole('button', { name: /rocksteady-1\.stl/i })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-testid="external-shape-viewport"]')).toHaveAttribute('data-preview-status', 'loaded');

  await page.getByRole('tab', { name: 'CROP', exact: true }).click();
  await expect(page.getByRole('complementary').getByText('rocksteady-1.stl')).toBeVisible();
  await expect(page.getByRole('region', { name: 'Existing plane crops' })).toContainText('PLANE 1');
  await expect(page.getByRole('region', { name: 'Existing plane crops' })).toContainText('KEEP ABOVE');
  await expect(page.locator('[data-testid="external-shape-viewport"]')).toBeVisible();
  await expect(page.getByText('PAIR PHONE')).toHaveCount(0);

  await page.getByRole('button', { name: 'Edit plane 1' }).click();
  await expect(page.getByText('EDIT PLANE 1 · POINTS 0/3')).toBeVisible();
  await expect(page.getByText('KEEP ABOVE PLANE')).toBeVisible();
  await page.getByRole('button', { name: 'CANCEL' }).click();
  await page.getByRole('button', { name: 'CUT PLANE' }).click();
  await expect(page.getByText('NEW PLANE · POINTS 0/3')).toBeVisible();
  await expect(page.locator('[data-testid="external-shape-viewport"] .viewer-host')).toHaveAttribute('data-crop-box-enabled', 'false');
  const canvas = page.locator('[data-testid="external-shape-viewport"] canvas');
  const bounds = await canvas.boundingBox();
  if (!bounds) throw new Error('external shape canvas missing');
  for (const point of [{ x: 0.46, y: 0.60 }, { x: 0.46, y: 0.60 }, { x: 0.46, y: 0.60 }]) {
    await canvas.click({ position: { x: bounds.width * point.x, y: bounds.height * point.y } });
  }
  await expect(page.getByText('NEW PLANE · POINTS 3/3')).toBeVisible();
  await expect(page.locator('[data-testid="capture-plane-overlay"]')).toHaveAttribute('data-point-count', '3');
  await expect(page.getByRole('button', { name: 'APPLY PLANE' })).toBeEnabled();
  await page.getByRole('button', { name: 'FLIP SIDE' }).click();
  await expect(page.getByText('KEEP BELOW PLANE')).toBeVisible();
  await page.getByRole('button', { name: 'CANCEL' }).click();
  await page.getByRole('button', { name: 'Remove plane 1' }).click();
  await expect(page.getByRole('region', { name: 'Existing plane crops' })).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__.some(
    (call: { cmd: string; args?: Record<string, any> }) =>
      call.cmd === 'apply_external_shape_edit' && call.args?.input?.edit?.action === 'removePlaneCrop' && call.args?.input?.edit?.cropNodeId === 19,
  ))).toBe(true);
});

test('Given legacy capture STL survived restart When user opens last capture Then preview returns for box crop', async ({ page }) => {
  await installCaptureShellMocks(page);
  await routeRestoredCaptureStl(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();

  await page.getByRole('button', { name: 'OPEN LAST CAPTURE' }).click();

  await expect(page.getByText('1,234 triangles')).toBeVisible();
  await expect(page.locator('[data-testid="capture-preview-viewport"]')).toHaveAttribute('data-preview-status', 'loaded');
  await expect(page.getByRole('button', { name: 'BOX CROP' })).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'adopt_latest_capture_run').length)).toBe(1);
});

test('Given durable capture belongs to task When task history opens Then capture can reopen with rotated pairing token', async ({ page }) => {
  await installCaptureShellMocks(page, { captureHistoryRun: true });
  await routeRestoredCaptureStl(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'PROJECTS' }).click();
  const project = page.locator('[data-window-id="projects"] .project-card').filter({ hasText: 'Finger fixture' });
  await project.getByRole('button', { name: 'OPEN' }).click();
  await page.getByRole('button', { name: 'DIALOGUE' }).click();

  await expect(page.getByRole('region', { name: 'Capture history' })).toBeVisible();
  await expect(page.getByText('Finger fixture capture')).toBeVisible();
  await page.getByRole('button', { name: 'OPEN CAPTURE' }).click();

  await expect(page.locator('[data-window-id="capture"]')).toBeVisible();
  await expect(page.locator('[data-testid="capture-preview-viewport"]')).toHaveAttribute('data-preview-status', 'loaded');
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'reopen_capture_run').length)).toBe(1);
  await page.getByRole('button', { name: 'ADD PHOTOS' }).click();
  await expect(page.getByText('48 frames retained; continue on same phone link')).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'resume_capture_session').length)).toBe(1);
});

test('Given LAN service fails When capture starts Then raw backend error remains visible', async ({ page }) => {
  await installCaptureShellMocks(page, { captureStartError: 'bind failed: address already in use' });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);

  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();

  await expect(page.getByText('bind failed: address already in use')).toBeVisible();
  await expect(page.locator('[data-testid="capture-panel"]')).toHaveAttribute('data-session-state', 'failed');
  const startIntent = await page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .find((call: { cmd: string }) => call.cmd === 'start_capture_session'));
  expect(startIntent.args).toEqual({ target: null });
});

test('Given reconstruction completes When desktop polls Then preview exposes inspect Apply and Commit', async ({ page }) => {
  await installCaptureShellMocks(page, { captureStatus: 'preview' });
  const triangles = [
    [[0, 0, 0], [2122, 0, 0], [0, 420, 0]],
    [[0, 0, 0], [0, 0, 2690], [2122, 0, 0]],
    [[0, 0, 0], [0, 420, 0], [0, 0, 2690]],
    [[2122, 0, 0], [0, 0, 2690], [0, 420, 0]],
  ];
  const previewStl = Buffer.alloc(84 + triangles.length * 50);
  previewStl.writeUInt32LE(triangles.length, 80);
  triangles.forEach((triangle, triangleIndex) => {
    triangle.forEach((vertex, vertexIndex) => {
      vertex.forEach((value, axis) => previewStl.writeFloatLE(
        value,
        84 + triangleIndex * 50 + 12 + vertexIndex * 12 + axis * 4,
      ));
    });
  });
  await page.route('**/tmp/capture-{model,box-crop}.stl*', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'model/stl',
      body: previewStl,
    });
  });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();

  await expect(page.getByText('1,234 triangles')).toBeVisible({ timeout: 3_000 });
  await expect(page.getByRole('button', { name: 'SWITCH TO PROJECT' })).toHaveCount(0);
  await expect(page.getByText('2.1 x 1.6 x 0.9 mm')).toBeVisible();
  await expect(page.getByRole('spinbutton', { name: 'Capture scale' })).toHaveValue('0.05');
  await expect(page.locator('[data-testid="capture-preview-viewport"]')).toHaveAttribute('data-preview-status', 'loaded');
  const previewCanvas = page.locator('[data-testid="capture-preview-viewport"] canvas');
  await expect(previewCanvas).toBeVisible();
  const captureWindow = page.locator('[data-window-id="capture"]');
  const windowBeforeViewerDrag = await captureWindow.boundingBox();
  const canvasBox = await previewCanvas.boundingBox();
  expect(windowBeforeViewerDrag).not.toBeNull();
  expect(canvasBox).not.toBeNull();
  await page.mouse.move(canvasBox!.x + canvasBox!.width * 0.5, canvasBox!.y + canvasBox!.height * 0.5);
  await page.mouse.down();
  await page.mouse.move(canvasBox!.x + canvasBox!.width * 0.5 + 40, canvasBox!.y + canvasBox!.height * 0.5 + 20);
  await page.mouse.up();
  const windowAfterViewerDrag = await captureWindow.boundingBox();
  expect(windowAfterViewerDrag?.x).toBe(windowBeforeViewerDrag?.x);
  expect(windowAfterViewerDrag?.y).toBe(windowBeforeViewerDrag?.y);
  await expect.poll(() => previewCanvas.evaluate((canvas: HTMLCanvasElement) => {
    const gl = canvas.getContext('webgl2') ?? canvas.getContext('webgl');
    if (!gl) return 0;
    const pixels = new Uint8Array(canvas.width * canvas.height * 4);
    gl.readPixels(0, 0, canvas.width, canvas.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    let litPixels = 0;
    for (let offset = 0; offset < pixels.length; offset += 4) {
      if (pixels[offset] > 25 || pixels[offset + 1] > 30 || pixels[offset + 2] > 45) litPixels += 1;
    }
    return litPixels;
  })).toBeGreaterThan(1_000);
  await expect(page.getByRole('button', { name: 'APPLY' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'COMMIT' })).toBeDisabled();
  await page.getByRole('button', { name: 'BOX CROP' }).click();
  await expect(page.getByRole('group', { name: 'Crop box transform' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'RESIZE BOX' })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'MOVE BOX' }).click();
  await expect(page.getByRole('button', { name: 'MOVE BOX' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'PREVIEW CROP' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'APPLY' })).toBeEnabled();
  await page.getByRole('button', { name: 'APPLY' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string; args?: { cropBounds?: unknown } }) =>
      call.cmd === 'prepare_capture_preview' && call.args?.cropBounds).length)).toBe(1);
  await expect(page.locator('[data-testid="capture-preview-viewport"]')).toHaveAttribute('data-preview-status', 'loaded');
  await expect(page.getByText('Capture solidify draft applied')).toBeVisible();
  const captureApply = await page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'apply_capture_preview'));
  expect(captureApply).toHaveLength(1);
  expect(captureApply[0].args).toEqual({ input: { runId: 'abc123' } });
  const applyCommands = await page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .map((call: { cmd: string }) => call.cmd));
  expect(applyCommands).not.toContain('macro_ast_source_map');
  expect(applyCommands).not.toContain('apply_manual_code');
  await expect(page.getByRole('button', { name: 'COMMIT' })).toBeEnabled();
  await page.getByRole('button', { name: 'COMMIT' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string; args?: Record<string, any> }) =>
      call.cmd === 'apply_manual_code' && call.args?.input?.persist === true).length)).toBe(1);
  const commit = await page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .find((call: { cmd: string; args?: Record<string, any> }) =>
      call.cmd === 'apply_manual_code' && call.args?.input?.persist === true));
  expect(commit.args.input.source).toContain(
    '(scale capture_scale_abc123 capture_scale_abc123 capture_scale_abc123 (solidify (import-stl "/tmp/capture-box-crop.stl")))',
  );
});

test('Given reconstruction fails When desktop polls Then raw error remains and no Apply appears', async ({ page }) => {
  await installCaptureShellMocks(page, { captureStatus: 'failed' });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();

  await expect(page.getByText('Object Capture request failed by test')).toBeVisible({ timeout: 3_000 });
  await expect(page.getByRole('button', { name: 'APPLY' })).toHaveCount(0);
  await page.getByRole('button', { name: 'RETRY RECONSTRUCTION' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'retry_capture_reconstruction').length)).toBe(1);
});

test('Given capture target advanced When Apply runs Then raw Rust stale error remains visible', async ({ page }) => {
  await installCaptureShellMocks(page, {
    captureStatus: 'preview',
    captureApplyError: 'Capture target source diverged: expected sha256:old, found sha256:new.',
  });
  await routeRestoredCaptureStl(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();
  await expect(page.getByRole('button', { name: 'APPLY' })).toBeVisible({ timeout: 3000 });

  await page.getByRole('button', { name: 'APPLY' }).click();

  await expect(page.getByText('Capture target source diverged: expected sha256:old, found sha256:new.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'COMMIT' })).toBeDisabled();
});

test('Given Rust capture apply remains pending When Apply is clicked twice Then one intent stays disabled', async ({ page }) => {
  await installCaptureShellMocks(page, { captureStatus: 'preview', captureApplyPending: true });
  await routeRestoredCaptureStl(page);
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();

  const apply = page.getByRole('button', { name: 'APPLY' });
  await expect(apply).toBeVisible({ timeout: 3000 });
  await apply.evaluate((button: HTMLButtonElement) => {
    button.click();
    button.click();
  });
  await expect(page.getByRole('button', { name: 'APPLYING' })).toBeDisabled();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'apply_capture_preview').length)).toBe(1);

  await page.evaluate(() => (window as any).__RESOLVE_CAPTURE_APPLY__());
  await expect(page.getByText('Capture solidify draft applied')).toBeVisible();
});

test('Given crop excludes the mesh When Apply runs Then raw preview and backend error remain', async ({ page }) => {
  await installCaptureShellMocks(page, {
    captureStatus: 'preview',
    cropError: 'Box crop contains no mesh triangles; raw capture preview retained.',
  });
  const previewStl = Buffer.alloc(84 + 50);
  previewStl.writeUInt32LE(1, 80);
  [[0, 0, 0], [40, 0, 0], [0, 30, 0]].forEach((vertex, vertexIndex) => {
    vertex.forEach((value, axis) => previewStl.writeFloatLE(value, 84 + 12 + vertexIndex * 12 + axis * 4));
  });
  await page.route('**/*capture-model.stl*', route => route.fulfill({
    status: 200,
    contentType: 'model/stl',
    body: previewStl,
  }));
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();
  const viewport = page.locator('[data-testid="capture-preview-viewport"]');
  await expect(viewport).toHaveAttribute('data-preview-status', 'loaded');

  await page.getByRole('button', { name: 'BOX CROP' }).click();
  await page.getByRole('button', { name: 'APPLY' }).click();

  await expect(page.getByText('Box crop contains no mesh triangles; raw capture preview retained.')).toBeVisible();
  await expect(viewport).toHaveAttribute('data-preview-status', 'loaded');
  await expect(page.getByRole('button', { name: 'COMMIT' })).toBeDisabled();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'add_manual_version').length)).toBe(0);
});

test('Given preview needs improvement When user adds photos Then same capture session resumes', async ({ page }) => {
  await installCaptureShellMocks(page, { captureStatus: 'preview' });
  await page.goto('/');
  await expect(page.locator('.boot-overlay')).toHaveCount(0);
  await page.getByRole('button', { name: 'Work with external shapes' }).click();
  await page.getByRole('button', { name: 'START CAPTURE' }).click();
  await expect(page.getByRole('button', { name: 'ADD PHOTOS' })).toBeVisible({ timeout: 3_000 });
  await page.getByRole('button', { name: 'ADD PHOTOS' }).click();
  await expect(page.getByText('48 frames retained; continue on same phone link')).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_CALLS__
    .filter((call: { cmd: string }) => call.cmd === 'resume_capture_session').length)).toBe(1);
});

test('Given synthetic phone camera When quality gates pass Then frames upload and capture finishes', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const captureServer = await installPhoneCaptureRoute(page);
  await page.goto('https://capture.test/capture/abc123');

  await page.getByRole('button', { name: 'Start camera' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_WAKE__.requests)).toBe(1);
  await expect(page.getByText('HOLD FOCUS')).toBeVisible({ timeout: 2_000 });
  await expect.poll(async () => Number.parseInt(await page.locator('#accepted').innerText(), 10), {
    timeout: 5_000,
  }).toBeGreaterThanOrEqual(20);
  await expect(page.getByText('0 pending')).toBeVisible();
  await page.setViewportSize({ width: 844, height: 390 });
  await expect(page.locator('#camera')).toHaveCSS('width', '844px');
  await expect(page.getByRole('button', { name: 'Build preview' })).toBeVisible();
  await page.getByRole('button', { name: 'Build preview' }).click();
  await expect(page.getByText('PREVIEW READY ON MAC')).toBeVisible({ timeout: 3_000 });
  await expect.poll(() => page.evaluate(() => (window as any).__CAPTURE_WAKE__.releases)).toBe(1);
  captureServer.requestMorePhotos();
  await expect(page.getByText('MORE PHOTOS REQUESTED')).toBeVisible({ timeout: 2_000 });
  await expect(page.getByRole('button', { name: 'Resume camera' })).toBeVisible();
});

test('Given photos were captured elsewhere When user uploads files Then reconstruction uses same capture session', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const captureServer = await installPhoneCaptureRoute(page);
  await page.goto('https://capture.test/capture/abc123');

  await expect(page.getByRole('button', { name: 'Upload photos' })).toBeVisible();
  await page.locator('#photo-files').setInputFiles(Array.from({ length: 20 }, (_, index) => ({
    name: `rig-${String(index + 1).padStart(2, '0')}.jpg`,
    mimeType: 'image/jpeg',
    buffer: Buffer.from([0xff, 0xd8, index + 1, 0xff, 0xd9]),
  })));

  await expect.poll(() => captureServer.frames.length, { timeout: 10_000 }).toBe(20);
  await expect.poll(async () => Number.parseInt(await page.locator('#accepted').innerText(), 10)).toBeGreaterThanOrEqual(20);
  await expect(page.getByRole('button', { name: 'Build preview' })).toBeEnabled();
  expect(await page.evaluate(() => (window as any).__CAPTURE_WAKE__.requests)).toBe(0);
  await page.getByRole('button', { name: 'Build preview' }).click();
  await expect(page.getByText('PREVIEW READY ON MAC')).toBeVisible({ timeout: 3_000 });
});

test('Given uploaded photo format is unsupported When file is selected Then raw upload error remains visible', async ({ page }) => {
  await installPhoneCaptureRoute(page);
  await page.goto('https://capture.test/capture/abc123');

  await page.locator('#photo-files').setInputFiles({
    name: 'rig-frame.heic',
    mimeType: 'image/heic',
    buffer: Buffer.from([1, 2, 3]),
  });

  await expect(page.getByText('UPLOAD FAILED')).toBeVisible();
  await expect(page.getByText('Error: Unsupported image MIME image/heic; use JPEG or PNG.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Upload photos' })).toBeVisible();
});

test('Given Safari denies camera When capture starts Then raw browser error and retry show', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installPhoneCaptureRoute(page, 'Permission denied by test');
  await page.goto('https://capture.test/capture/abc123');

  await page.getByRole('button', { name: 'Start camera' }).click();
  await expect(page.getByText('CAMERA FAILED')).toBeVisible();
  await expect(page.getByText('NotAllowedError: Permission denied by test')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
});

test('Given LAN drops after frame acceptance When it returns Then queued digest uploads once', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const captureServer = await installPhoneCaptureRoute(page, undefined, true);
  await page.goto('https://capture.test/capture/abc123');

  await page.getByRole('button', { name: 'Start camera' }).click();
  await expect(page.getByText('TRANSFER PENDING')).toBeVisible({ timeout: 3_000 });
  await expect(page.locator('#pending')).toHaveText(/[1-9]\d* pending/);
  captureServer.restoreNetwork();
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await expect(page.locator('#accepted')).toHaveText(/[1-9]\d* accepted/);
  const digests = captureServer.frames.map(frame => frame.contentDigest);
  expect(new Set(digests).size).toBe(digests.length);
});
