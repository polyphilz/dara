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

test('code-language listbox survives a held pointer click and applies selection', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const editor = page.getByTestId('front-editor')
  const front = editor.getByRole('textbox', { name: 'Front' })
  await front.fill('const answer = 42')
  await editor.getByRole('button', { name: 'Code block' }).click()

  const language = editor.getByRole('button', {
    name: 'Code language: Plain text',
  })
  await language.click({ delay: 75 })
  const listbox = page.getByRole('listbox', { name: 'Code language' })
  await expect(listbox).toBeVisible()

  await page.getByRole('option', { name: 'TypeScript' }).click({ delay: 75 })
  await expect(
    editor.getByRole('button', { name: 'Code language: TypeScript' }),
  ).toBeVisible()
})

test('code-block Command-A selection hugs text and blank lines', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  await front.click()
  await page.keyboard.type('```')
  await page.keyboard.type('first line')
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  await page.keyboard.type('second longer line')

  const code = page.locator('.dara-code-block').first()
  const lines = code.locator('.cm-line')
  await expect(lines).toHaveCount(3)
  await page.keyboard.press('ControlOrMeta+a')

  const selectionMetrics = await code.evaluate((block) => {
    const content = block.querySelector<HTMLElement>('.cm-content')
    const codeLines = Array.from(
      block.querySelectorAll<HTMLElement>('.cm-line'),
    )
    if (!content || codeLines.length !== 3) {
      throw new Error('Expected a three-line CodeMirror document')
    }
    return {
      blankLineWidth: codeLines[1].getBoundingClientRect().width,
      contentWidth: content.getBoundingClientRect().width,
      selectionColor: getComputedStyle(
        codeLines[0],
        '::selection',
      ).backgroundColor,
      textLineWidth: codeLines[0].getBoundingClientRect().width,
    }
  })

  expect(selectionMetrics.textLineWidth).toBeLessThan(
    selectionMetrics.contentWidth,
  )
  expect(selectionMetrics.blankLineWidth).toBeGreaterThan(0)
  expect(selectionMetrics.blankLineWidth).toBeLessThan(
    selectionMetrics.textLineWidth,
  )
  expect(selectionMetrics.selectionColor).toBe('rgb(41, 59, 84)')
})

test('Tab indents inside a code block and Escape releases focus', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  await front.click()
  await page.keyboard.type('```')
  await page.keyboard.type('pass')
  const code = page.locator('.dara-code-block-editor .cm-content').first()
  await expect(code).toBeVisible()

  await page.keyboard.press('Tab')
  await expect(code).toHaveText('  pass')
  await page.keyboard.press('Shift+Tab')
  await expect(code).toHaveText('pass')

  await page.keyboard.press('Escape')
  await expect(
    page.locator('.dara-code-block-editor .cm-editor.cm-focused'),
  ).toHaveCount(0)
})
