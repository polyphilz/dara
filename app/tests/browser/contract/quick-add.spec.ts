import { BrowserScenarioId } from '../harness/scenarios.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import { expect, test } from '../fixtures/test.ts'
import { DaraIpcCommand } from '../../../src/lib/tauri-contracts.ts'

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

test('code-block indentation renders one WebKit caret at the indent', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  await front.click()
  await page.keyboard.type('```')
  await page.keyboard.type('first line')
  await page.keyboard.press('Enter')

  const code = page.locator('.dara-code-block-editor .cm-content').first()
  await expect(code).toBeVisible()
  await page.keyboard.press('Tab')
  await page.keyboard.press('Shift+Tab')
  await page.keyboard.press('Tab')

  const cursor = page.locator('.cm-cursor-primary')
  await expect(cursor).toHaveCount(1)
  const caret = await code.evaluate((content) => {
    const cursorElement = content
      .closest('.cm-editor')
      ?.querySelector<HTMLElement>('.cm-cursor-primary')
    const indentedLine = content.querySelectorAll<HTMLElement>('.cm-line')[1]
    if (!cursorElement || !indentedLine) {
      throw new Error('Expected an indented line and CodeMirror cursor')
    }
    const cursorBounds = cursorElement.getBoundingClientRect()
    const lineBounds = indentedLine.getBoundingClientRect()
    return {
      cursorCount: content
        .closest('.cm-editor')
        ?.querySelectorAll('.cm-cursor-primary').length,
      cursorLeft: cursorBounds.left,
      lineLeft: lineBounds.left,
      nativeCaretColor: getComputedStyle(content).caretColor,
    }
  })

  expect(caret.cursorCount).toBe(1)
  expect(caret.cursorLeft).toBeGreaterThan(caret.lineLeft)
  expect(caret.nativeCaretColor).toBe('rgba(0, 0, 0, 0)')
})

test('selected images suppress WebKit native selection tint', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })

  const webkitUserSelect = await front.evaluate((editor) => {
    const image = document.createElement('figure')
    image.className = 'dara-editor-image dara-editor-image-selected'
    editor.append(image)
    const value = getComputedStyle(image).getPropertyValue(
      '-webkit-user-select',
    )
    image.remove()
    return value
  })

  expect(webkitUserSelect).toBe('none')
})

test('editor links open through the external URL command in WebKit', async ({
  page,
}) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const editor = page.getByTestId('front-editor')
  const front = editor.getByRole('textbox', { name: 'Front' })
  await front.fill('Open docs')
  await front.press('ControlOrMeta+a')
  await editor.getByRole('button', { name: 'Link' }).click()
  await editor
    .getByRole('textbox', { name: 'Link URL' })
    .fill('https://example.com/docs')
  await editor.getByRole('button', { name: 'Done' }).click()

  const link = front.locator('a[href="https://example.com/docs"]')
  await expect(link).toHaveText('Open docs')
  await link.click()

  const snapshot = await page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
  expect(snapshot.commands).toContainEqual({
    command: DaraIpcCommand.OpenExternalUrl,
    payload: { url: 'https://example.com/docs' },
  })
})
