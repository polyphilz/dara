import { Appearance } from '../../../src/settings/types.ts'
import { BrowserHarnessSurface, BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

for (const appearance of [Appearance.Light, Appearance.Dark]) {
  test(`shared controls in ${appearance.toLowerCase()} appearance`, async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 720 })
    await page.goto(
      `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}&appearance=${appearance}`,
    )
    await page.getByRole('button', { name: 'Catalog choice: Second option' }).focus()
    await expect(page.locator('.visual-catalog')).toHaveScreenshot(
      `controls-${appearance.toLowerCase()}.png`,
    )
  })
}

test('shared controls open listbox state', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await page.getByRole('button', { name: 'Catalog choice: Second option' }).press('Enter')
  await expect(page.getByRole('listbox', { name: 'Catalog choice' })).toBeVisible()
  await expect(page.locator('body')).toHaveScreenshot('controls-open-listbox.png')
})

test('shared controls shortcut recording state', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await page.getByRole('button', { name: /Catalog shortcut:/ }).click()
  await expect(page.getByText('Press shortcut…')).toBeVisible()
  await expect(page.locator('.visual-catalog')).toHaveScreenshot('controls-recording.png')
})
