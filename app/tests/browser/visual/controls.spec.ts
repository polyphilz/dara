import { Appearance } from '../../../src/settings/types.ts'
import { BrowserHarnessSurface, BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

/*
 * The design-system catalog is captured as two focused snapshots. Each scope
 * fits the established 1000x720 viewport, so a reviewer can read every
 * specimen at full size instead of scanning one oversized image.
 */
const TYPOGRAPHY_SCOPE = '#catalog-type-group'
const CONTROLS_SCOPE = '#catalog-control-group'

for (const appearance of [Appearance.Light, Appearance.Dark]) {
  test(`typography scale in ${appearance.toLowerCase()} appearance`, async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 720 })
    await page.goto(
      `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}&appearance=${appearance}`,
    )
    await page.evaluate(async () => {
      await document.fonts.ready
    })
    await expect(page.locator(TYPOGRAPHY_SCOPE)).toHaveScreenshot(
      `typography-${appearance.toLowerCase()}.png`,
    )
  })

  test(`shared controls in ${appearance.toLowerCase()} appearance`, async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 720 })
    await page.goto(
      `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}&appearance=${appearance}`,
    )
    await page.getByRole('button', { name: 'Catalog choice: Second option' }).focus()
    await expect(page.locator(CONTROLS_SCOPE)).toHaveScreenshot(
      `controls-${appearance.toLowerCase()}.png`,
    )
  })
}

test('shared controls open listbox state', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await page.evaluate(async () => {
    await document.fonts.ready
  })
  await page.getByRole('button', { name: 'Catalog choice: Second option' }).press('Enter')
  await expect(page.getByRole('listbox', { name: 'Catalog choice' })).toBeVisible()
  await expect(page).toHaveScreenshot('controls-open-listbox.png')
})

test('shared controls shortcut recording state', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await page.getByRole('button', { name: /Catalog shortcut:/ }).click()
  await expect(page.getByText('Press shortcut…')).toBeVisible()
  await expect(page.locator(CONTROLS_SCOPE)).toHaveScreenshot('controls-recording.png')
})
