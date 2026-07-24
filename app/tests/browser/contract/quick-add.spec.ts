import { BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Quick Add editor focus and real key events work in WebKit', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  await expect(front).toBeFocused()
  await front.pressSequentially('WebKit contract')
  await page.keyboard.press('Tab')
  await expect(page.getByRole('textbox', { name: 'Back' })).toBeFocused()
})
