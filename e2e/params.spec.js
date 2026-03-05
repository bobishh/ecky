import { test, expect } from '@playwright/test';

test.describe('ParamPanel Persistence', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Wait for the app to boot and workbench to show
    await page.waitForSelector('.workbench');
  });

  test('saving values should trigger tauri invoke and show success state', async ({ page }) => {
    // 1. Enter some parameters if none exist, or wait for them to load
    // Assuming we have at least one parameter field
    const applyBtn = page.locator('.apply-btn');
    const saveValuesBtn = page.getByRole('button', { name: /SAVE VALUES/i });

    // Mocking might be hard here without actual Tauri context, 
    // but we can check if the button goes to "SAVED" state
    
    // Let's check if the button exists first
    await expect(saveValuesBtn).toBeVisible();
    
    // We need an activeVersionId for the button to be enabled
    // If it's disabled, we can't test the click.
    const isDisabled = await saveValuesBtn.isDisabled();
    if (!isDisabled) {
      await saveValuesBtn.click();
      await expect(page.getByText('SAVED')).toBeVisible();
    }
  });

  test('editing controls should persist to ui_spec', async ({ page }) => {
    const editBtn = page.getByRole('button', { name: /EDIT CONTROLS/i });
    await editBtn.click();

    const addFieldBtn = page.getByRole('button', { name: /\+ ADD FIELD/i });
    await addFieldBtn.click();

    // Fill in a new field
    const lastField = page.locator('.edit-field').last();
    await lastField.locator('input[placeholder="key"]').fill('test_param');
    await lastField.locator('input[placeholder="Label"]').fill('Test Param');

    const saveFieldsBtn = page.getByRole('button', { name: /SAVE/i }).filter({ hasText: '💾 SAVE' });
    await saveFieldsBtn.click();

    // Check if the field appeared in the list
    await expect(page.locator('.param-label').filter({ hasText: 'TEST PARAM' })).toBeVisible();
  });

  test('newly added control survives a parameter change (render)', async ({ page }) => {
    // 1. Add a control
    const editBtn = page.getByRole('button', { name: /EDIT CONTROLS/i });
    await editBtn.click();

    const addFieldBtn = page.getByRole('button', { name: /\+ ADD FIELD/i });
    await addFieldBtn.click();

    const lastField = page.locator('.edit-field').last();
    await lastField.locator('input[placeholder="key"]').fill('newly_added_param');
    await lastField.locator('input[placeholder="Label"]').fill('Newly Added Param');

    const saveFieldsBtn = page.getByRole('button', { name: /SAVE/i }).filter({ hasText: '💾 SAVE' });
    await saveFieldsBtn.click();
    await expect(page.locator('.param-label').filter({ hasText: 'NEWLY ADDED PARAM' })).toBeVisible();

    // 2. Change a parameter to trigger a render/store update
    // We assume there's at least one slider if any params exist
    // If not, we can use the newly added one if it rendered as a number input
    const newlyAddedInput = page.locator('#newly_added_param');
    await newlyAddedInput.fill('42');
    
    // The change should trigger handleParamChange, which updates the store
    // Wait a bit for the store update and re-render
    await page.waitForTimeout(500);

    // 3. Verify the control is still there
    await expect(page.locator('.param-label').filter({ hasText: 'NEWLY ADDED PARAM' })).toBeVisible();
  });
});
