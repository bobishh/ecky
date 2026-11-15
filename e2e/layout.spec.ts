import { test, expect, type Locator } from '@playwright/test';

async function numericZIndex(locator: Locator) {
  return locator.evaluate((element) => Number.parseInt(window.getComputedStyle(element).zIndex || '0', 10));
}

test.describe('Layout', () => {
  test('workbench layout shows the viewport', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('.viewport-area')).toBeVisible();
  });

  test('dock opens parameter and project windows', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true }).click();
    await expect(page.locator('[data-window-id="params"]')).toBeVisible();
    await page.getByRole('button', { name: 'Projects', exact: true }).click();
    await expect(page.locator('[data-window-id="projects"]')).toBeVisible();
  });

  test('Given overlapping floating windows When a lower window is clicked Then it becomes focused and topmost', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Projects', exact: true }).click();
    const projectsWindow = page.locator('[data-window-id="projects"]');
    await expect(projectsWindow).toBeVisible();

    await page.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true }).click();
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
    await page.getByRole('button', { name: /(PARAMS|Parameters)/i, exact: true }).click();
    await expect(page.locator('[data-window-id="params"] .window-resize-handle')).toBeVisible();
  });

  test('Given a floating window When maximized and restored Then the dock stays reachable and its bounds return', async ({ page }) => {
    await page.goto('/');
    const dock = page.getByRole('toolbar', { name: 'Workbench tools' });
    const launcher = dock.getByRole('button', { name: 'Work with external shapes' });
    await launcher.click();

    const shapes = page.locator('[data-window-id="capture"]');
    const maximize = shapes.getByRole('button', { name: 'Maximize window' });
    await expect(maximize).toBeVisible();
    const floatingBounds = await shapes.boundingBox();
    await maximize.click();

    const maximizedBounds = await shapes.boundingBox();
    const dockBounds = await dock.boundingBox();
    expect(maximizedBounds).toMatchObject({ x: 0, y: 0, width: page.viewportSize()?.width });
    expect(maximizedBounds!.y + maximizedBounds!.height).toBeLessThan(dockBounds!.y);
    await expect(shapes.locator('.window-resize-handle')).toBeHidden();
    await dock.getByRole('button', { name: 'Projects', exact: true }).click();
    await expect(page.locator('[data-window-id="projects"]')).toBeVisible();
    await launcher.click();

    await shapes.getByRole('button', { name: 'Restore window' }).click();
    expect(await shapes.boundingBox()).toEqual(floatingBounds);
    await expect(shapes.locator('.window-resize-handle')).toBeVisible();
  });
});
