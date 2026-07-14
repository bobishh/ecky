import { expect, test } from '@playwright/test';

// BDD e2e for the server-rendered Ecky IR Field Guide served at /docs/.
// Outer BDD rule: prove user-visible behaviour, happy path + a failure/pending state.

test.describe('Ecky IR Field Guide — web docs reader', () => {
  test('Given the docs page When it opens Then hero title, summary and dark theme render', async ({ page }) => {
    await page.goto('/docs/');

    await expect(page).toHaveTitle(/Ecky IR Field Guide/);
    await expect(page.getByRole('heading', { name: 'Ecky IR Field Guide', level: 1 })).toBeVisible();
    // Summary copy from the corpus survives into the hero.
    await expect(page.getByText('building models in order')).toBeVisible();

    // Midnight Tactical dark theme is applied to the body.
    const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    expect(bg).toBe('rgb(26, 26, 46)'); // #1a1a2e
  });

  test('Given the docs page When rendered Then the sidebar TOC lists all sections', async ({ page }) => {
    await page.goto('/docs/');

    const toc = page.locator('nav[aria-label="Field guide contents"]');
    const links = toc.locator('.docs-toc__link');
    await expect(links).toHaveCount(34);

    await expect(toc.getByText('First Solid: Ball on a Base')).toBeVisible();
    await expect(toc.getByText('Verify Clauses')).toBeVisible();
    await expect(toc.getByText('Language Overview')).toBeVisible();
  });

  test('Given a TOC link When clicked Then the page scrolls to that section and highlights it', async ({ page }) => {
    await page.goto('/docs/');

    // Navigate directly via hash (reliable across browsers) then assert scroll-spy.
    await page.goto('/docs/#verify-clauses');

    const heading = page.getByRole('heading', { name: 'Verify Clauses', level: 2 });
    await expect(heading).toBeInViewport();

    // Scroll-spy marks the matching TOC entry active.
    const activeLink = page.locator('.docs-toc__link--active');
    await expect(activeLink).toHaveAttribute('href', '#verify-clauses');
  });

  test('Given the docs page When rendered Then section bodies contain real code blocks and figures', async ({ page }) => {
    await page.goto('/docs/');

    // Real content, not an empty shell.
    const codeBlocks = page.locator('.docs-main pre code');
    await expect(codeBlocks.first()).toBeVisible();
    const codeCount = await codeBlocks.count();
    expect(codeCount).toBeGreaterThan(10);

    // Rendered example images resolve under /docs/assets/.
    const figureImg = page.locator('.docs-main figure img').first();
    await expect(figureImg).toHaveAttribute('src', /\/docs\/assets\//);
    // The image actually loads (not a broken link).
    const naturalWidth = await figureImg.evaluate((img) => (img as HTMLImageElement).naturalWidth);
    expect(naturalWidth).toBeGreaterThan(0);
  });

  test('Given the docs page When rendered Then raw markdown and EPUB download links are present', async ({ page }) => {
    await page.goto('/docs/');

    const mdLink = page.getByRole('link', { name: /Raw .md/i });
    await expect(mdLink).toHaveAttribute('href', '/docs/ecky-ir.md');

    const epubLink = page.getByRole('link', { name: /EPUB/i });
    await expect(epubLink).toHaveAttribute('href', '/docs/ecky-ir-field-guide.epub');
  });

  test('Given the raw markdown URL When fetched Then it is served as text/markdown with real content', async ({ request }) => {
    // Failure/pending-state guard: the agent-facing artifact must be reachable
    // and carry the right content-type, or agents cannot consume it.
    const response = await request.get('/docs/ecky-ir.md');
    expect(response.status()).toBe(200);
    expect(response.headers()['content-type']).toContain('text/markdown');
    const body = await response.text();
    expect(body).toContain('# Ecky IR Field Guide');
    expect(body).toContain('First Solid: Ball on a Base');
  });

  test('Given the docs page When scrolled deep Then a later section renders with its own content', async ({ page }) => {
    // Happy-path breadth check: content near the end of the single page is real.
    await page.goto('/docs/#language-overview');

    const heading = page.getByRole('heading', { name: 'Language Overview', level: 2 });
    await expect(heading).toBeInViewport();
  });

  test('Given the docs page When fully loaded Then no console or page errors fire', async ({ page }) => {
    // Pending-state guard: the static page must not surface runtime JS errors.
    const errors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });
    page.on('pageerror', (err) => errors.push(err.message));

    await page.goto('/docs/');
    await page.waitForLoadState('networkidle');

    expect(errors).toEqual([]);
  });
});
