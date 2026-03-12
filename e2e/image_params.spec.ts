import { test, expect } from '@playwright/test';

test.describe('Image Parameter Types', () => {
  test('renders image fields and allows interaction', async ({ page }) => {
    // 1. Setup mock that returns an image field
    await page.route('http://localhost:3000/api/mock/generation', async (route) => {
      await route.fulfill({
        json: {
          title: 'Lithophane Mock',
          versionName: 'V1',
          response: 'Here is your lithophane.',
          interactionMode: 'design',
          macroCode: 'print("litho")',
          uiSpec: {
            fields: [
              {
                type: 'image',
                key: 'source_image',
                label: 'Upload Lithophane Photo',
              },
            ],
          },
          initialParams: {},
          postProcessing: {
            displacement: {
              image_param: 'source_image',
              projection: 'cylindrical',
              depth_mm: 3.0,
              invert: false
            }
          }
        },
      });
    });

    await page.goto('/');
    
    // Clear history and skip onboarding if it appears
    await page.evaluate(() => { window.localStorage.clear(); });
    await page.reload();
    
    const skipBtn = page.getByRole('button', { name: 'SKIP' });
    if (await skipBtn.isVisible()) {
      await skipBtn.click();
    }
    
    await page.fill('textarea.prompt-input', 'make a lithophane (mock)');
    await page.keyboard.press('Enter');

    // 3. Wait for the generation to finish and UI to render
    await expect(page.locator('.param-panel')).toBeVisible({ timeout: 10000 });
    
    // 4. Verify Image Field is rendered
    const imageFieldLabel = page.getByText('Upload Lithophane Photo');
    await expect(imageFieldLabel).toBeVisible();

    const uploadBtn = page.getByRole('button', { name: 'Select Image...' });
    await expect(uploadBtn).toBeVisible();
    
    // 5. Simulate file selection (mocking Tauri dialog since we can't open native OS dialogs in Playwright)
    await page.evaluate(() => {
      // We inject a global mock for the dialog plugin
      window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
      window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
        if (cmd === 'plugin:dialog|open') {
          return '/Users/test/Desktop/cool_photo.jpg';
        }
        return null;
      };
    });

    // 6. Click the button and check if path updates
    await uploadBtn.click();
    
    // The button text should update to the basename of the file
    await expect(page.getByRole('button', { name: 'cool_photo.jpg' })).toBeVisible();
  });
});
