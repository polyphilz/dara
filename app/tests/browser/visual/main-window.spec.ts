import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'
import { Appearance } from '../../../src/settings/types.ts'

test('Home populated surface', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await expect(page.locator('.main-window')).toHaveScreenshot('home-populated.png')
})

test('formatted Review surface', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()
  const italicPhrase = page.locator('.review-card em').filter({
    hasText: 'could',
  })
  await expect(italicPhrase).toHaveCSS('font-style', 'italic')
  await expect(italicPhrase).toHaveCSS(
    'font-family',
    /JetBrains Mono Variable/,
  )
  await page.keyboard.press('Space')
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()
  await expect(
    page.locator('.review-card em').filter({
      hasText: 'Both forms return',
    }),
  ).toHaveCSS('font-style', 'italic')
  await expect(page.locator('.main-window')).toHaveScreenshot(
    'review-basic-revealed.png',
  )
})

for (const appearance of [Appearance.Light, Appearance.Dark]) {
  test(`Cloze review question distinguishes deletions in ${appearance.toLowerCase()} mode`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1000, height: 720 })
    await page.goto(
      `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewCloze}&surface=${BrowserHarnessSurface.Main}&appearance=${appearance}`,
    )
    await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
    await expect(
      page.getByRole('note', { name: 'Hidden cloze deletion' }),
    ).toHaveCount(2)
    await expect(
      page.locator('.review-card em').filter({ hasText: 'personal' }),
    ).toHaveCSS('font-style', 'italic')
    await expect(
      page.locator('.review-card strong').filter({
        hasText: 'a Python function',
      }),
    ).toHaveCSS('font-weight', '700')
    await expect(page.locator('.main-window')).toHaveScreenshot(
      `review-cloze-question-${appearance.toLowerCase()}.png`,
    )
  })
}
