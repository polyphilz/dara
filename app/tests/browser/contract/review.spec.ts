import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('review reveal and grade focus work in WebKit', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()
  await page.keyboard.press('Space')
  await expect(page.getByRole('button', { name: /Good/ })).toHaveClass(
    /grade-focused/,
  )
  await page.keyboard.press('Tab')
  await expect(page.getByRole('button', { name: /Easy/ })).toHaveClass(
    /grade-focused/,
  )
})
