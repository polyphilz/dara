import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Home populated and Review revealed surfaces', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await expect(page.locator('.main-window')).toHaveScreenshot('home-populated.png')
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await page.keyboard.press('Space')
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()
  await expect(page.locator('.main-window')).toHaveScreenshot(
    'review-basic-revealed.png',
  )
})
