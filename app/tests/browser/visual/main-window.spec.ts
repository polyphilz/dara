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
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()
  await page.keyboard.press('Space')
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()
  await expect(page.locator('.main-window')).toHaveScreenshot(
    'review-basic-revealed.png',
  )
})

test('Cloze review question distinguishes deletions in light and dark mode', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewCloze}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(
    page.getByRole('note', { name: 'Hidden cloze deletion' }),
  ).toHaveCount(2)
  await expect(page.locator('.main-window')).toHaveScreenshot(
    'review-cloze-question-light.png',
  )

  await page.emulateMedia({ colorScheme: 'dark' })
  await expect(page.locator('.main-window')).toHaveScreenshot(
    'review-cloze-question-dark.png',
  )
})
