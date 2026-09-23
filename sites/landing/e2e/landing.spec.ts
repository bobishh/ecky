import { expect, test } from '@playwright/test';

/**
 * Landing page e2e — BDD Given/When/Then, user-visible behavior.
 * The landing is a separate static Vite project from the Tauri app, but it
 * reuses the canonical genome from src/lib/genie. These tests gate the two
 * things that can silently break: (a) the landing shell renders with the right
 * structure + CTAs, and (b) the mascot / STL viewer mount without console
 * errors (genome import / Three.js scene build failures would surface as
 * errors on load).
 */

test.describe('Ecky landing', () => {
  test('Given the README tone When the landing opens Then the product and optional AI are explained plainly', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Parametric parts from code.' })).toBeVisible();
    await expect(page.locator('.hero-lede')).toContainText('optional AI assistance');
    await expect(page.locator('body')).not.toContainText(/\bshit\b/i);
    await expect(page.locator('#workflow')).toContainText('including failed ones');
  });

  test('Given the app mark When favicon renders Then it matches the canonical Ecky face', async ({ page }) => {
    await page.goto('/favicon.svg');

    const silhouette = page.locator('#ecky-silhouette');
    const eyes = page.locator('[data-ecky-feature="eye"]');
    const mouth = page.locator('[data-ecky-feature="mouth"]');

    await expect(silhouette).toBeVisible();
    await expect(eyes).toHaveCount(2);
    await expect(mouth).toHaveCount(1);
    await expect(mouth).toHaveAttribute('points');
    await expect(page.locator('[data-ecky-feature="smile"], [data-ecky-feature="node"]')).toHaveCount(0);
  });

  test('Given no packaged app release When page opens Then local AI-assisted CAD and inspectable outputs lead', async ({ page }) => {
    const errors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/');

    await expect(page.locator('.nav')).toContainText('Ecky CAD');
    const hero = page.locator('.hero');
    await expect(hero.getByRole('heading', { name: 'Parametric parts from code.' })).toBeVisible();
    await expect(hero.getByText(/EXPERIMENTAL DESKTOP CAD/)).toBeVisible();
    await expect(hero.getByText(/optional AI assistance/i)).toBeVisible();
    await expect(hero.getByTestId('model-workbench')).toBeVisible();

    await expect(page.getByRole('link', { name: 'Download', exact: true })).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Download ↗', exact: true })).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Releases ↗', exact: true })).toHaveCount(0);
    await expect(page.locator('a[href*="/releases"]')).toHaveCount(0);
    await expect(hero.getByRole('link', { name: 'Read the chapters' })).toHaveAttribute('href', '/docs/chapters/');
    await expect(hero.getByRole('link', { name: 'Inspect working models' })).toHaveAttribute('href', '#models');
    await expect(hero.getByRole('link', { name: 'DOWNLOAD ZIP' })).toBeVisible();

    expect(errors, 'page opened with no console errors').toEqual([]);
  });

  test('Given mascot zone When page loads Then WebGL canvas mounts without errors', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(`pageerror: ${err.message}`));
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/');
    const canvas = page.locator('.mascot canvas');
    await expect(canvas).toBeVisible();

    await expect.poll(async () => {
      const ok = await canvas.evaluate((el: HTMLCanvasElement) => el.width > 0 && el.height > 0);
      return ok;
    }, { message: 'canvas has nonzero backing size', timeout: 10_000 }).toBe(true);

    expect(errors, 'mascot mounted without genome/renderer errors').toEqual([]);
  });

  test('Given reduced motion When the hero settles Then mascot and scrolling remain still', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/');

    const canvas = page.locator('.mascot canvas');
    await expect(canvas).toBeVisible();
    const firstFrame = await canvas.screenshot();
    await page.waitForTimeout(500);
    const secondFrame = await canvas.screenshot();
    expect(secondFrame).toEqual(firstFrame);
    await expect.poll(() => page.evaluate(() => getComputedStyle(document.documentElement).scrollBehavior)).toBe('auto');
  });

  test('Given a phone viewport When Ecky is dragged Then the visible mascot rotates without blocking page scroll', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/');

    const mascot = page.locator('.hero-mascot');
    const canvas = mascot.locator('.mascot canvas');
    await expect(canvas).toBeVisible();
    await expect(mascot).toHaveCSS('pointer-events', 'auto');
    await expect(canvas).toHaveCSS('touch-action', 'none');

    await canvas.scrollIntoViewIfNeeded();
    const box = await canvas.boundingBox();
    expect(box, 'mobile mascot has a measurable canvas').not.toBeNull();
    expect(box?.x).toBeGreaterThanOrEqual(0);
    expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(390);

    const before = await canvas.screenshot();
    await page.mouse.move((box?.x ?? 0) + (box?.width ?? 0) * 0.25, (box?.y ?? 0) + (box?.height ?? 0) * 0.5);
    await page.mouse.down();
    await page.mouse.move((box?.x ?? 0) + (box?.width ?? 0) * 0.75, (box?.y ?? 0) + (box?.height ?? 0) * 0.3, { steps: 6 });
    await page.mouse.up();
    const after = await canvas.screenshot();
    expect(after, 'drag produces a new rendered mascot pose').not.toEqual(before);

    const scrollBefore = await page.evaluate(() => window.scrollY);
    await page.mouse.wheel(0, 500);
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(scrollBefore);
  });

  test('Given project notes When read Then setup, history and provider boundaries are explicit', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('.hero-summary')).toContainText(/build from source; expect bugs/i);
    await expect(page.locator('#source pre')).toContainText('(model');
    await expect(page.locator('#workflow')).toContainText('No AI account is needed');
    await expect(page.locator('#workflow')).toContainText('remote provider receives');
    await expect(page.locator('#learn')).toContainText('Node.js, Rust');
    await expect(page.getByRole('link', { name: /Function reference/ })).toHaveAttribute('href', '/docs/');
  });

  test('Given a 390px viewport When navigation renders Then every link stays on one line', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/');

    const nav = page.locator('.nav');
    const measurements = await nav.evaluate((element) => ({
      height: element.getBoundingClientRect().height,
      links: Array.from(element.querySelectorAll('a')).map((link) => {
        const rect = link.getBoundingClientRect();
        return { text: link.textContent?.trim(), height: rect.height, lineHeight: Number.parseFloat(getComputedStyle(link).lineHeight) };
      }),
    }));

    expect(measurements.height, 'compact mobile navigation').toBeLessThanOrEqual(68);
    for (const link of measurements.links) {
      expect(link.height, `${link.text} remains one line`).toBeLessThanOrEqual(link.lineHeight * 1.25);
    }
  });

  test('Given social crawlers When the landing metadata is read Then a canonical share card is complete', async ({ page, request }) => {
    await page.goto('/');

    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://ecky-cad.com/');
    await expect(page.locator('meta[property="og:image"]')).toHaveAttribute('content', 'https://ecky-cad.com/og-image.png');
    await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute('content', 'summary_large_image');
    await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute('content', '#1a1a2e');

    const robots = await request.get('/robots.txt');
    expect(robots.headers()['content-type']).toContain('text/plain');
    expect(await robots.text()).toContain('Sitemap: https://ecky-cad.com/sitemap.xml');

    const sitemap = await request.get('/sitemap.xml');
    expect(sitemap.headers()['content-type']).toMatch(/xml/);
    expect(await sitemap.text()).toContain('<loc>https://ecky-cad.com/</loc>');

    const indexNowKey = await request.get('/indexnow-key.txt');
    expect(indexNowKey.headers()['content-type']).toContain('text/plain');
    expect((await indexNowKey.text()).trim()).toBe('69a8ca5615ffe76f7e56f6a662beaf6a');
  });

  test('Given the showcase section When page loads Then the live STL viewer mounts the current phone case', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(`pageerror: ${err.message}`));
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/');
    // The vignette renders the real exported STL; interaction/content are
    // specified in case-workbench.spec.ts.
    const caseStudy = page.locator('#case-study');
    await expect(caseStudy.getByRole('heading', { name: 'Parametric parts from code.' })).toBeVisible();

    const viewer = caseStudy.locator('.viewer canvas');
    await expect(viewer).toBeVisible();
    await expect.poll(async () => {
      const ok = await viewer.evaluate((el: HTMLCanvasElement) => el.width > 0 && el.height > 0);
      return ok;
    }, { message: 'STL viewer canvas mounted', timeout: 10_000 }).toBe(true);

    // The STL is served (not 404): gate on the loading hint disappearing.
    await expect.poll(async () => {
      const stillLoading = await caseStudy.locator('.viewer-load').count();
      return stillLoading;
    }, { message: 'STL part finished loading', timeout: 15_000 }).toBe(0);

    expect(errors, 'STL viewer loaded the mesh without errors').toEqual([]);
  });

  test('Given learning routes When the page opens Then chapters and reference are linked', async ({ page }) => {
    await page.goto('/');

    const learn = page.locator('#learn');
    await expect(learn.getByRole('link', { name: /Start with a bracket/ })).toHaveAttribute('href', '/docs/chapters/');
    await expect(learn.getByRole('link', { name: /Function reference/ })).toHaveAttribute('href', '/docs/');
  });
});
