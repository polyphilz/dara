import { BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Quick Add Basic empty surface', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const surface = page.getByRole('region', { name: 'Quick add' })
  await expect(surface).toBeVisible()
  await expect(surface).toHaveScreenshot('quick-add-basic-empty.png')
})
