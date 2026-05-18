import { expect, test } from '@playwright/test';

test('Given static docs entry When page opens Then file-backed index and article render without app shell chrome', async ({ page }) => {
  await page.goto('/ecky-ir/');

  await expect(page.getByRole('heading', { name: 'Ecky Language Docs' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Language Overview' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Forms and Structure' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'PROJECTS' })).toHaveCount(0);

  await page.getByRole('button', { name: 'Forms and Structure' }).click();

  await expect(page.getByRole('heading', { name: 'Forms and Structure' })).toBeVisible();
  await expect(page.getByText('This is top-level authoring grammar.')).toBeVisible();
});

test('Given docs route When page opens Then manifest-driven docs index and article render', async ({ page }) => {
  await page.goto('/docs/ecky-ir');

  await expect(page.getByRole('heading', { name: 'Ecky Language Docs' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Language Overview' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Verify Clauses' })).toBeVisible();

  await page.getByRole('button', { name: 'Verify Clauses' }).click();

  await expect(page.getByRole('heading', { name: 'Verify Clauses' })).toBeVisible();
  await expect(page.getByText('Use verify when source should declare structural expectations explicitly.')).toBeVisible();
  await expect(page.locator('pre').first()).toContainText('(verify');
  await expect(page.locator('pre').filter({ hasText: 'clearance min-distance' }).first()).toBeVisible();
});

test('Given docs route When pending article opens Then pending state stays visible', async ({ page }) => {
  await page.goto('/docs/ecky-ir');

  const pendingArticle = page.getByRole('button', { name: /Constraint Dojo/i });
  await expect(pendingArticle).toBeVisible();
  await expect(pendingArticle).toContainText(/pending/i);

  await pendingArticle.click();

  await expect(page.getByRole('heading', { name: 'Constraint Dojo' })).toBeVisible();
  await expect(page.locator('.docs-status--pending')).toContainText('Pending');
});
