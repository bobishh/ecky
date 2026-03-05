import { test, expect } from '@playwright/test';
import path from 'path';

test.describe('Drag and Drop', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Wait for the app to boot
    await page.waitForSelector('.prompt-container');
  });

  test('dragging a file over shows the overlay', async ({ page }) => {
    const container = page.locator('.prompt-container');
    
    // Trigger dragover via web event
    await page.evaluate(() => {
      const container = document.querySelector('.prompt-container');
      const dragOverEvent = new DragEvent('dragover', {
        bubbles: true,
        cancelable: true,
        dataTransfer: new DataTransfer()
      });
      container.dispatchEvent(dragOverEvent);
    });

    // Check for overlay
    const overlay = page.locator('.drag-overlay');
    await expect(overlay).toBeVisible();
    await expect(page.getByText('DROP TO ATTACH REFERENCES')).toBeVisible();

    // Trigger dragleave
    await page.evaluate(() => {
      const container = document.querySelector('.prompt-container');
      const dragLeaveEvent = new DragEvent('dragleave', {
        bubbles: true,
        cancelable: true
      });
      container.dispatchEvent(dragLeaveEvent);
    });
    await expect(overlay).not.toBeVisible();
  });

  test('dropping an image adds it to attachments', async ({ page }) => {
    const container = page.locator('.prompt-container');
    
    // We simulate a drop event with a mock file
    // Note: In a real browser environment (like Playwright), we don't get absolute paths
    // but our component handles the fallback to filename which is enough to test the UI.
    await page.evaluate(() => {
      const container = document.querySelector('.prompt-container');
      const dataTransfer = new DataTransfer();
      const file = new File([''], 'test-image.jpg', { type: 'image/jpeg' });
      dataTransfer.items.add(file);
      
      const dropEvent = new DragEvent('drop', {
        bubbles: true,
        cancelable: true,
        dataTransfer
      });
      container.dispatchEvent(dropEvent);
    });

    // Check if attachment appeared
    await expect(page.locator('.attachment-item')).toBeVisible();
    await expect(page.locator('.att-name')).toContainText('test-image.jpg');
    await expect(page.locator('.att-type')).toContainText('🖼️ IMG');
  });

  test('dropping a CAD file adds it correctly', async ({ page }) => {
    await page.evaluate(() => {
      const container = document.querySelector('.prompt-container');
      const dataTransfer = new DataTransfer();
      const file = new File([''], 'part.stl', { type: 'application/sla' });
      dataTransfer.items.add(file);
      
      const dropEvent = new DragEvent('drop', {
        bubbles: true,
        cancelable: true,
        dataTransfer
      });
      container.dispatchEvent(dropEvent);
    });

    // Check if attachment appeared
    await expect(page.locator('.attachment-item')).toBeVisible();
    await expect(page.locator('.att-name')).toContainText('part.stl');
    await expect(page.locator('.att-type')).toContainText('📐 CAD');
  });
});
