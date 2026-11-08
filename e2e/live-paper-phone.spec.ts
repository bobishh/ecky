import { expect, test, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';

async function installPhoneCamera(page: Page, rejectConstraintUpdate = false) {
  const assets = new URL('../src-tauri/assets/', import.meta.url);
  await page.route('**/live-paper?*', async (route) => route.fulfill({
    contentType: 'text/html', body: await readFile(new URL('live_paper_client.html', assets), 'utf8'),
  }));
  await page.route('**/capture-assets/live_paper_client.mjs', async (route) => route.fulfill({
    contentType: 'text/javascript', body: await readFile(new URL('live_paper_client.mjs', assets), 'utf8'),
  }));
  await page.route('**/api/live-paper/proof/offer', (route) => route.fulfill({
    contentType: 'application/json', body: JSON.stringify({ type: 'answer', sdp: 'phone-layout-fixture' }),
  }));
  await page.addInitScript((rejectConstraintUpdate) => {
    const canvas = document.createElement('canvas');
    canvas.width = 1280;
    canvas.height = 720;
    const context = canvas.getContext('2d')!;
    context.fillStyle = '#ffffff'; context.fillRect(0, 0, 1280, 720);
    for (const [x, y, color] of [[0, 0, '#ff0000'], [1180, 0, '#00ff00'], [0, 620, '#0000ff'], [1180, 620, '#000000']] as const) {
      context.fillStyle = color; context.fillRect(x, y, 100, 100);
    }
    const media = canvas.captureStream(15);
    const track = media.getVideoTracks()[0]!;
    const state = { constraints: [] as MediaTrackConstraints[], media };
    (window as any).__PHONE_CAMERA__ = state;
    track.applyConstraints = async (constraints) => {
      state.constraints.push(constraints ?? {});
      if (rejectConstraintUpdate) throw new DOMException('Camera size is fixed', 'OverconstrainedError');
    };
    navigator.mediaDevices.getUserMedia = async (constraints) => {
      state.constraints.push(constraints?.video as MediaTrackConstraints);
      return media;
    };
    class LayoutPeer extends EventTarget {
      iceGatheringState = 'complete';
      connectionState = 'connected';
      localDescription: any;
      addTransceiver() { return { setCodecPreferences() {} }; }
      async createOffer() { return { type: 'offer', sdp: 'phone-layout-fixture' }; }
      async setLocalDescription(description: any) { this.localDescription = description; }
      async setRemoteDescription() { this.dispatchEvent(new Event('connectionstatechange')); }
      close() { this.connectionState = 'closed'; }
    }
    window.RTCPeerConnection = LayoutPeer as any;
  }, rejectConstraintUpdate);
}

test('landscape camera in a portrait phone shows every corner by default with controls overlaid', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installPhoneCamera(page);
  await page.goto('/live-paper?session=proof&token=test-token');
  await page.getByRole('button', { name: 'Start camera', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Streaming live paper');
  const video = page.locator('#camera');
  await expect(video).toHaveJSProperty('videoWidth', 1280);
  await expect(video).toHaveJSProperty('videoHeight', 720);
  await expect(video).toHaveCSS('object-fit', 'contain');
  expect(await video.boundingBox()).toMatchObject({ x: 0, y: 0, width: 390, height: 844 });
  await page.screenshot({ path: testInfo.outputPath('landscape-camera-in-portrait.png') });
  await page.getByRole('button', { name: 'Help', exact: true }).click();
  await expect(page.getByRole('checkbox', { name: 'Show whole camera frame' })).toBeChecked();
  await page.getByRole('checkbox', { name: 'Show whole camera frame' }).uncheck();
  await expect(video).toHaveCSS('object-fit', 'cover');
  await page.getByRole('checkbox', { name: 'Show whole camera frame' }).check();
  await expect(video).toHaveCSS('object-fit', 'contain');
});

test('phone orientation changes update the same camera track without reconnecting or cropping', async ({ page }) => {
  await page.setViewportSize({ width: 844, height: 390 });
  await installPhoneCamera(page);
  await page.goto('/live-paper?session=proof&token=test-token');
  await page.getByRole('button', { name: 'Start camera', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Streaming live paper');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => (window as any).__PHONE_CAMERA__.constraints.at(-1)?.height?.ideal)).toBe(1280);
  expect(await page.evaluate(() => (window as any).__PHONE_CAMERA__.constraints.at(-1)?.aspectRatio?.ideal)).toBe(9 / 16);
  await page.setViewportSize({ width: 844, height: 390 });
  await expect.poll(() => page.evaluate(() => (window as any).__PHONE_CAMERA__.constraints.at(-1)?.width?.ideal)).toBe(1280);
  await expect(page.locator('#camera')).toHaveCSS('object-fit', 'contain');
  await expect(page.getByRole('button', { name: 'Stop camera', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Stop camera', exact: true }).click();
  expect(await page.evaluate(() => (window as any).__PHONE_CAMERA__.media.getVideoTracks()[0].readyState)).toBe('ended');
});

test('camera refusing a new portrait resolution keeps the full frame and working Stop control', async ({ page }) => {
  await page.setViewportSize({ width: 844, height: 390 });
  await installPhoneCamera(page, true);
  await page.goto('/live-paper?session=proof&token=test-token');
  await page.getByRole('button', { name: 'Start camera', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Streaming live paper');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole('status')).toContainText('The whole frame remains visible');
  await expect(page.locator('#camera')).toHaveCSS('object-fit', 'contain');
  await expect(page.getByRole('button', { name: 'Stop camera', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Stop camera', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Camera stopped');
});
