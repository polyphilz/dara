import type { Page } from '@playwright/test'
import { DaraIpcCommand } from '../../../src/lib/tauri-contracts.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('review held Space reveals once and grades only after keyup', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(page.locator('.review-card')).toContainText(
    'Given this Unicode code point',
  )

  await page.keyboard.down(' ')
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()
  await expect(page.getByRole('button', { name: /Good/ })).toHaveClass(
    /grade-focused/,
  )
  await page.keyboard.down(' ')
  const beforeRelease = await snapshot(page)
  expect(beforeRelease.recordedGrades).toHaveLength(0)

  await page.keyboard.up(' ')
  await page.keyboard.press('Space')
  await expect(
    page.getByRole('heading', { name: 'Caught up for now' }),
  ).toBeVisible()
  const afterGrade = await snapshot(page)
  expect(afterGrade.recordedGrades).toHaveLength(1)
  expect(afterGrade.recordedGrades[0]?.review.grade).toBe(3)
  expect(
    afterGrade.commands.filter(
      ({ command }) => command === DaraIpcCommand.RecordGrade,
    ),
  ).toHaveLength(1)
})

test('review grade focus clamps, direct grades, and Meta+Z undoes', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()
  await page.keyboard.press('Space')
  const again = page.getByRole('button', { name: /Again/ })
  const good = page.getByRole('button', { name: /Good/ })
  await expect(good).toHaveClass(/grade-focused/)

  await page.keyboard.press('Shift+Tab')
  await page.keyboard.press('Shift+Tab')
  await page.keyboard.press('Shift+Tab')
  await expect(again).toHaveClass(/grade-focused/)
  await page.keyboard.press('Tab')
  await page.keyboard.press('Tab')
  await page.keyboard.press('Tab')
  await page.keyboard.press('Tab')
  await expect(page.getByRole('button', { name: /Easy/ })).toHaveClass(
    /grade-focused/,
  )

  await page.keyboard.press('4')
  await expect(
    page.getByRole('heading', { name: 'Caught up for now' }),
  ).toBeVisible()
  expect((await snapshot(page)).recordedGrades[0]?.review.grade).toBe(4)

  await page.keyboard.press('Meta+z')
  await expect(page.locator('.review-card')).toContainText(
    'Given this Unicode code point',
  )
  await expect(page.getByRole('group', { name: 'Grade this card' })).toHaveCount(0)
  const commands = (await snapshot(page)).commands.map(({ command }) => command)
  expect(commands).toContain(DaraIpcCommand.UndoLastGrade)
})

test('review scratchpad keeps rich work through reveal, then stays open and clears for the next card', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 900 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewTwoCards}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()

  const toggle = page.getByRole('button', { name: 'Open a scratchpad' })
  await expect(toggle).toHaveAttribute('title', 'Open a scratchpad')
  await expect(toggle).toHaveAttribute('aria-expanded', 'false')
  await expect(page.getByRole('region', { name: 'Scratchpad' })).toHaveCount(0)
  await toggle.focus()
  await page.keyboard.press('Space')
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()

  const scratchpad = page.getByRole('region', { name: 'Scratchpad' })
  const editor = scratchpad.getByTestId('scratchpad-editor')
  const textbox = editor.getByRole('textbox', { name: 'Scratchpad' })
  await textbox.evaluate((element) => {
    element.dataset.reviewEditorIdentity = 'preserved'
  })
  const reviewCard = page.locator('.review-card')
  const hideScratchpad = page.getByRole('button', { name: 'Hide scratchpad' })
  await expect(hideScratchpad).toHaveAttribute('title', 'Hide scratchpad')
  const [wideCardBounds, wideScratchpadBounds, wideToggleBounds] = await Promise.all([
    reviewCard.boundingBox(),
    scratchpad.boundingBox(),
    hideScratchpad.boundingBox(),
  ])
  expect(wideCardBounds).not.toBeNull()
  expect(wideScratchpadBounds).not.toBeNull()
  expect(wideToggleBounds).not.toBeNull()
  expect(wideToggleBounds!.y).toBe(wideCardBounds!.y)
  expect(wideScratchpadBounds!.x).toBeGreaterThan(
    wideCardBounds!.x + wideCardBounds!.width,
  )
  await expect(editor.getByRole('button', { name: 'Inline math' })).toBeVisible()
  await expect(editor.getByRole('button', { name: 'Display math' })).toBeVisible()
  await expect(editor.getByRole('button', { name: 'Insert image' })).toHaveCount(0)
  for (const label of ['Bold', 'Italic', 'Strikethrough', 'Link', 'Block quote']) {
    await expect(editor.getByRole('button', { name: label })).toHaveCount(0)
  }

  await textbox.click()
  await page.keyboard.type('path = Path(__file__) / "data" / "file.txt"')
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()
  await expect(page.getByRole('group', { name: 'Grade this card' })).toHaveCount(0)

  await page.getByRole('button', { name: 'Hide scratchpad' }).click()
  await expect(scratchpad).toBeHidden()
  await page.getByRole('button', { name: 'Open a scratchpad' }).click()
  await expect(textbox).toHaveText(
    'path = Path(__file__) / "data" / "file.txt"',
  )

  await editor.getByRole('button', { name: 'Code block' }).click()
  await editor
    .getByRole('button', { name: 'Code language: Plain text' })
    .click()
  await page.getByRole('option', { name: 'Python' }).click()
  await expect(
    editor.getByRole('button', { name: 'Code language: Python' }),
  ).toBeVisible()

  await page.getByRole('button', { name: 'Reveal answer' }).click()
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()
  const code = scratchpad.locator('.dara-code-block-editor .cm-content')
  await expect(code).toHaveText(
    'path = Path(__file__) / "data" / "file.txt"',
  )

  await code.click()
  await page.keyboard.press('End')
  await page.keyboard.press('Enter')
  await page.keyboard.type('print(path) 1234')
  await page.keyboard.insertText(
    `\n${Array.from(
      { length: 48 },
      (_, index) => `scratch line ${index + 1}`,
    ).join('\n')}`,
  )
  const cappedScratchpadBounds = await scratchpad.boundingBox()
  expect(cappedScratchpadBounds).not.toBeNull()
  expect(
    cappedScratchpadBounds!.y + cappedScratchpadBounds!.height,
  ).toBeLessThanOrEqual(853)
  expect(
    await scratchpad.locator('.rich-text-editor-surface').evaluate(
      (surface) => surface.scrollHeight > surface.clientHeight,
    ),
  ).toBe(true)
  expect((await snapshot(page)).recordedGrades).toHaveLength(0)
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()

  await page.getByRole('button', { name: /Good/ }).click()
  await expect(reviewCard).toContainText(
    'What should happen to an open scratchpad after grading the previous card?',
  )
  const resetScratchpad = page.getByRole('region', { name: 'Scratchpad' })
  await expect(resetScratchpad).toBeVisible()
  await expect(
    page.getByRole('button', { name: 'Hide scratchpad' }),
  ).toHaveAttribute('aria-expanded', 'true')
  await expect(
    resetScratchpad.getByRole('textbox', { name: 'Scratchpad' }),
  ).toHaveText('')
  await expect(
    resetScratchpad.getByRole('textbox', { name: 'Scratchpad' }),
  ).toHaveAttribute('data-review-editor-identity', 'preserved')
  await expect(resetScratchpad.locator('.dara-code-block')).toHaveCount(0)

  await page.setViewportSize({ width: 800, height: 900 })
  const [narrowCardBounds, narrowScratchpadBounds] = await Promise.all([
    reviewCard.boundingBox(),
    resetScratchpad.boundingBox(),
  ])
  expect(narrowCardBounds).not.toBeNull()
  expect(narrowScratchpadBounds).not.toBeNull()
  expect(narrowScratchpadBounds!.y).toBeGreaterThan(
    narrowCardBounds!.y + narrowCardBounds!.height,
  )

  await page.setViewportSize({ width: 600, height: 900 })
  await expect(page.getByRole('button', { name: 'Hide scratchpad' })).toHaveCount(0)
  await expect(page.getByRole('region', { name: 'Scratchpad' })).toHaveCount(0)

  await page.setViewportSize({ width: 800, height: 900 })
  await expect(page.getByRole('button', { name: 'Hide scratchpad' })).toBeVisible()
  await expect(resetScratchpad).toBeVisible()
})

async function snapshot(page: Page) {
  return page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
}
