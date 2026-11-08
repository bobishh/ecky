import { expect, test, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';

async function installPhoneCamera(page: Page, permissionDenied = false) {
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
  await page.addInitScript((permissionDenied) => {
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
    const state = { captureRequests: 0, reconfigurations: 0, offers: 0, media };
    (window as any).__PHONE_CAMERA__ = state;
    track.applyConstraints = async () => { state.reconfigurations += 1; };
    navigator.mediaDevices.getUserMedia = async () => {
      state.captureRequests += 1;
      if (permissionDenied) throw new DOMException('Camera permission denied', 'NotAllowedError');
      return media;
    };
    class LayoutPeer extends EventTarget {
      iceGatheringState = 'complete';
      connectionState = 'connected';
      localDescription: any;
      addTransceiver() { return { setCodecPreferences() {} }; }
      async createOffer() { state.offers += 1; return { type: 'offer', sdp: 'phone-layout-fixture' }; }
      async setLocalDescription(description: any) { this.localDescription = description; }
      async setRemoteDescription() { this.dispatchEvent(new Event('connectionstatechange')); }
      close() { this.connectionState = 'closed'; }
    }
    window.RTCPeerConnection = LayoutPeer as any;
  }, permissionDenied);
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
  expect(await video.evaluate((element: HTMLVideoElement) => (element.srcObject as MediaStream).getVideoTracks()[0]!.contentHint)).toBe('detail');
  expect(await video.boundingBox()).toMatchObject({ x: 0, y: 0, width: 390, height: 844 });
  await page.screenshot({ path: testInfo.outputPath('landscape-camera-in-portrait.png') });
  await page.getByRole('button', { name: 'Help', exact: true }).click();
  await expect(page.getByRole('checkbox', { name: 'Show whole camera frame' })).toBeChecked();
  await page.getByRole('checkbox', { name: 'Show whole camera frame' }).uncheck();
  await expect(video).toHaveCSS('object-fit', 'cover');
  await page.getByRole('checkbox', { name: 'Show whole camera frame' }).check();
  await expect(video).toHaveCSS('object-fit', 'contain');
});

test('phone orientation changes keep the camera track and frame intact without reconnecting', async ({ page }) => {
  await page.setViewportSize({ width: 844, height: 390 });
  await installPhoneCamera(page);
  await page.goto('/live-paper?session=proof&token=test-token');
  await page.getByRole('button', { name: 'Start camera', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Streaming live paper');
  await page.setViewportSize({ width: 390, height: 844 });
  const video = page.locator('#camera');
  await expect.poll(() => video.boundingBox()).toMatchObject({ width: 390, height: 844 });
  await page.setViewportSize({ width: 844, height: 390 });
  await expect.poll(() => video.boundingBox()).toMatchObject({ width: 844, height: 390 });
  await expect(video).toHaveCSS('object-fit', 'contain');
  expect(await page.evaluate(() => {
    const state = (window as any).__PHONE_CAMERA__;
    return { captureRequests: state.captureRequests, reconfigurations: state.reconfigurations, offers: state.offers };
  })).toEqual({ captureRequests: 1, reconfigurations: 0, offers: 1 });
  await expect(page.getByRole('button', { name: 'Stop camera', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Stop camera', exact: true }).click();
  expect(await page.evaluate(() => (window as any).__PHONE_CAMERA__.media.getVideoTracks()[0].readyState)).toBe('ended');
});

test('camera permission denial leaves the viewfinder and controls ready to retry', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installPhoneCamera(page, true);
  await page.goto('/live-paper?session=proof&token=test-token');
  const start = page.getByRole('button', { name: 'Start camera', exact: true });
  const bounds = await start.boundingBox();
  await start.click();
  await expect(page.getByRole('status')).toContainText('Camera permission denied');
  await expect(page.locator('#camera')).toHaveCSS('object-fit', 'contain');
  await expect(start).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Stop camera', exact: true })).toBeDisabled();
  expect(await start.boundingBox()).toEqual(bounds);
});
