import { test, expect, type Locator } from '@playwright/test';

async function numericZIndex(locator: Locator) {
  return locator.evaluate((element) => Number.parseInt(window.getComputedStyle(element).zIndex || '0', 10));
}

test.describe('Layout', () => {
  test('workbench layout has all major panels', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('.viewport-area')).toBeVisible();
    await expect(page.getByRole('button', { name: 'PROJECTS' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'PARAMS' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'DIALOGUE' })).toBeVisible();
  });

  test('dock opens parameter and project windows', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('[data-window-id="params"]')).toBeVisible();
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    await expect(page.locator('[data-window-id="projects"]')).toBeVisible();
  });

  test('Given overlapping floating windows When a lower window is clicked Then it becomes focused and topmost', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PROJECTS' }).click();
    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow).toBeVisible();

    await page.getByRole('button', { name: 'PARAMS' }).click();
    const paramsWindow = page.locator('[data-window-id="params"]');
    await expect(paramsWindow).toBeVisible();
    await expect(paramsWindow).toHaveClass(/window--focused/);
    expect(await numericZIndex(paramsWindow)).toBeGreaterThan(await numericZIndex(projectsWindow));

    await projectsWindow.click({ position: { x: 24, y: 24 } });
    await expect(projectsWindow).toHaveClass(/window--focused/);
    await expect(paramsWindow).not.toHaveClass(/window--focused/);
    expect(await numericZIndex(projectsWindow)).toBeGreaterThan(await numericZIndex(paramsWindow));
  });

  test('vertical and horizontal resizers exist', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'PARAMS' }).click();
    await expect(page.locator('[data-window-id="params"] .window-resize-handle')).toBeVisible();
  });
});
