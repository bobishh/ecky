import { expect, test } from '@playwright/test';

test('Given static docs entry When page opens Then file-backed index and article render without app shell chrome', async ({ page }) => {
  await page.goto('/ecky-ir/index.html');

  await expect(page.getByRole('heading', { name: 'Ecky IR Field Guide' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'First Solid: Ball on a Base' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Final Model: Integrated Film Adapter Open Helicoid v9' })).toBeVisible();
  await expect(page.getByTestId('workbench-bottom-dock')).toHaveCount(0);

  await page.getByRole('button', { name: 'First Solid: Ball on a Base' }).click();

  await expect(page.getByRole('heading', { name: 'First Solid: Ball on a Base' })).toBeVisible();
  await expect(page.getByText('Start with the smallest complete')).toBeVisible();
  await expect(page.locator('img[alt*="First Solid"]').first()).toHaveAttribute('src', /\/docs\/assets\/01-first-solid-01\.png$/);
});

test('Given docs route When page opens Then manifest-driven docs index and article render', async ({ page }) => {
  await page.goto('/docs/ecky-ir');

  await expect(page.getByRole('heading', { name: 'Ecky IR Field Guide' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'First Solid: Ball on a Base' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Verify Clauses' })).toBeVisible();

  await page.getByRole('button', { name: 'Verify Clauses' }).click();

  await expect(page.getByRole('heading', { name: 'Verify Clauses' })).toBeVisible();
  await expect(page.getByText('Use verify when source should declare structural expectations explicitly.')).toBeVisible();
  await expect(page.locator('pre').first()).toContainText('clearance min-distance');
});

test('Given docs route When constraint dojo opens Then no pending state leaks into docs chrome', async ({ page }) => {
  await page.goto('/docs/ecky-ir');

  const article = page.getByRole('button', { name: /Constraint Dojo/i });
  await expect(article).toBeVisible();
  await expect(article).not.toContainText(/pending/i);

  await article.click();

  await expect(page.getByRole('heading', { name: 'Constraint Dojo' })).toBeVisible();
  await expect(page.locator('.docs-status--pending')).toHaveCount(0);
  await expect(page.locator('.docs-article')).not.toContainText(/pending/i);
});

test('Given docs route When epub action pressed Then book download starts', async ({ page }) => {
  await page.goto('/docs/ecky-ir');

  const downloadPromise = page.waitForEvent('download');
  await page.getByRole('button', { name: 'DOWNLOAD EPUB' }).click();
  const download = await downloadPromise;

  await expect(page.getByRole('button', { name: 'DOWNLOAD EPUB' })).toBeVisible();
  expect(download.suggestedFilename()).toBe('ecky-ir-field-guide.epub');
});
