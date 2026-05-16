import { test, expect } from '@playwright/test';

test.describe('Prompt Panel', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'DIALOGUE' }).click();
  });

  test('prompt textarea exists and is focusable', async ({ page }) => {
    const textarea = page.locator('.prompt-input');
    await expect(textarea).toBeVisible();
    await textarea.focus();
    await expect(textarea).toBeFocused();
  });

  test('send button is disabled when prompt is empty', async ({ page }) => {
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await expect(sendBtn).toBeDisabled();
  });

  test('send button enables when text is entered', async ({ page }) => {
    const textarea = page.locator('.prompt-input');
    await textarea.fill('Create a simple box');
    const sendBtn = page.locator('button:has-text("PROCESS")');
    await expect(sendBtn).toBeEnabled();
  });

  test('attach reference button exists', async ({ page }) => {
    const attachBtn = page.locator('button:has-text("ATTACH REFERENCE")');
    await expect(attachBtn).toBeVisible();
  });

  test('dialogue window opens when no thread is active', async ({ page }) => {
    await expect(page.locator('[data-window-id="dialogue"]')).toContainText('Dialogue');
  });
});
