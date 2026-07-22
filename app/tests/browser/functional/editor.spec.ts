import { CardContentType } from '../../../src/review/contracts.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import { BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('rich-text selection saves canonical Markdown through the public result', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  await front.fill('retrieval')
  await front.press('ControlOrMeta+a')
  const bold = page
    .getByTestId('front-editor')
    .getByRole('button', { name: 'Bold' })
  await bold.click()
  await expect(bold).toHaveAttribute('aria-pressed', 'true')
  await page.getByRole('textbox', { name: 'Back' }).fill('practice')
  await page.keyboard.press('Meta+Enter')

  const snapshot = await page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
  expect(snapshot.cards[0]?.content).toEqual({
    backMd: 'practice',
    frontMd: '**retrieval**',
    source: null,
    type: CardContentType.Basic,
  })
})
