import { test, expect } from '@playwright/test';

test.describe('Prompt Panel', () => {
  test('prompt textarea exists and is focusable', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const textarea = page.locator('.prompt-input');
    await expect(textarea).toBeVisible();
    await textarea.focus();
    await expect(textarea).toBeFocused();
  });

  test('send button is disabled when prompt is empty', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await expect(sendBtn).toBeDisabled();
  });

  test('send button enables when text is entered', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const textarea = page.locator('.prompt-input');
    await textarea.fill('Create a simple box');
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await expect(sendBtn).toBeEnabled();
  });

  test('attach reference button exists', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    const attachBtn = page.locator('button:has-text("ATTACH REFERENCE")');
    await expect(attachBtn).toBeVisible();
  });

  test('dialogue header shows "New Session" when no thread is active', async ({ page }) => {
    await page.goto('/');
    await page.waitForTimeout(2000);
    await expect(page.locator('.pane-header:has-text("DIALOGUE")')).toContainText('New Session');
  });
});
