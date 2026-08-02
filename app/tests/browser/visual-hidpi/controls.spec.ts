import { BrowserHarnessSurface, BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('shared control one-device-pixel details at DPR 2', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await page.getByRole('button', { name: 'Catalog choice: Second option' }).focus()
  // Scoped to the control group: this check is about one-device-pixel borders
  // and focus rings, not the typography specimens.
  await expect(page.locator('#catalog-control-group')).toHaveScreenshot(
    'controls-dpr2.png',
    { scale: 'device' },
  )
})
