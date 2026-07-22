import AxeBuilder from '@axe-core/playwright'
import { BrowserHarnessSurface, BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('shared control catalog has no detectable accessibility violations', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await expect(page.getByRole('heading', { name: 'Dara control states' })).toBeVisible()
  const results = await new AxeBuilder({ page }).analyze()
  expect(results.violations).toEqual([])
})
