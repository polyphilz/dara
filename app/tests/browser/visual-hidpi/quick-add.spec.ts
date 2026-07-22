import { BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Quick Add focus outline at DPR 2', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const surface = page.getByRole('region', { name: 'Quick add' })
  await expect(page.getByRole('textbox', { name: 'Front' })).toBeFocused()
  await expect(surface).toHaveScreenshot('quick-add-focus-dpr2.png', {
    scale: 'device',
  })
})
