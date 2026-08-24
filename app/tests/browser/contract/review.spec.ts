import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

const APP_FONT_FAMILY = 'JetBrains Mono Variable'

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

test('review loads genuine JetBrains Mono emphasis faces in WebKit', async ({
  page,
}) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()

  const emphasis = page.locator('.review-card em').filter({ hasText: 'could' })
  await expect(emphasis).toHaveCSS('font-family', /JetBrains Mono Variable/)
  await expect(emphasis).toHaveCSS('font-style', 'italic')

  const loadedStyles = await page.evaluate(async (fontFamily) => {
    await document.fonts.ready
    return [...document.fonts]
      .filter(
        (face) =>
          face.family === fontFamily && face.status === 'loaded',
      )
      .map((face) => face.style)
  }, APP_FONT_FAMILY)
  expect(loadedStyles).toContain('normal')
  expect(loadedStyles).toContain('italic')

  await page.keyboard.press('Space')
  const boldItalic = page.locator('.review-card em').filter({
    hasText: 'Both forms return',
  })
  await expect(boldItalic).toHaveCSS('font-style', 'italic')
  await expect(boldItalic.locator('strong')).toHaveCSS('font-weight', '700')
})
