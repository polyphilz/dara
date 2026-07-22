import { CardContentType } from '../../../src/review/contracts.ts'
import { DaraEvent, DaraIpcCommand } from '../../../src/lib/tauri-contracts.ts'
import { BrowserScenarioId } from '../harness/scenarios.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import { expect, test } from '../fixtures/test.ts'

test('Quick Add Basic completes its keyboard-first save contract', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )

  const front = page.getByRole('textbox', { name: 'Front' })
  const back = page.getByRole('textbox', { name: 'Back' })
  const source = page.getByRole('textbox', { name: /Source/ })
  const add = page.getByRole('button', { name: /Add/ })

  await expect(front).toBeFocused()
  await front.fill('Why does spaced repetition work?')
  await page.keyboard.press('Tab')
  await expect(back).toBeFocused()
  await back.fill('It schedules retrieval near the edge of forgetting.')
  await page.keyboard.press('Tab')
  await expect(source).toBeFocused()
  await source.fill('  Learning notes  ')
  await page.keyboard.press('Tab')
  await expect(add).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(source).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(back).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(front).toBeFocused()

  await page.keyboard.press('Meta+Enter')
  await expect(front).toHaveText('')

  const result = await page.evaluate(() => {
    const api = (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__
    return { events: api.events(), snapshot: api.snapshot() }
  })
  expect(result.snapshot.cards).toHaveLength(1)
  expect(result.snapshot.cards[0]?.content).toEqual({
    type: CardContentType.Basic,
    frontMd: 'Why does spaced repetition work?',
    backMd: 'It schedules retrieval near the edge of forgetting.',
    source: 'Learning notes',
  })
  expect(result.snapshot.commands.map(({ command }) => command)).toEqual([
    DaraIpcCommand.CreateCardContent,
    DaraIpcCommand.DismissQuickAdd,
  ])
  expect(result.snapshot.dismissedQuickAdd).toBe(1)
  expect(result.events).toEqual([
    { event: DaraEvent.CardCreated, payload: undefined },
  ])
})

test('Quick Add validation preserves the draft and focuses the missing editor', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  const back = page.getByRole('textbox', { name: 'Back' })
  await front.fill('Draft question')

  await page.keyboard.press('Meta+Enter')
  await expect(page.getByRole('alert')).toHaveText('Add an answer before saving.')
  await expect(back).toBeFocused()
  await expect(front).toHaveText('Draft question')
  expect((await backendSnapshot(page)).cards).toHaveLength(0)
})

test('Quick Add save failure preserves content and retry creates one card', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddCreateFailsOnce}`,
  )
  const front = page.getByRole('textbox', { name: 'Front' })
  const back = page.getByRole('textbox', { name: 'Back' })
  await front.fill('A draft that must survive')
  await back.fill('The backend failed once')

  await page.keyboard.press('Meta+Enter')
  await expect(page.getByRole('alert')).toHaveText(
    'The deterministic save fault fired.',
  )
  await expect(front).toHaveText('A draft that must survive')
  await expect(back).toHaveText('The backend failed once')
  expect((await backendSnapshot(page)).cards).toHaveLength(0)

  const add = page.getByRole('button', { name: /Add/ })
  await expect(add).toBeEnabled()
  await add.click()
  await expect(front).toHaveText('')
  const result = await backendSnapshot(page)
  expect(result.cards).toHaveLength(1)
  expect(result.dismissedQuickAdd).toBe(1)
})

test('Quick Add card types are keyboard reachable and nested Escape does not dismiss', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  await expect(page.getByRole('textbox', { name: 'Front' })).toBeFocused()
  const trigger = page.getByRole('button', { name: 'Card type: Basic' })
  await trigger.focus()
  await trigger.press('ArrowDown')
  const listbox = page.getByRole('listbox', { name: 'Card type' })
  await expect(listbox).toBeVisible()
  await expect(page.getByRole('option', { name: 'Basic' })).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: 'Card type: Cloze' })).toBeVisible()

  await page.getByRole('button', { name: 'Card type: Cloze' }).press('ArrowDown')
  await page.keyboard.press('End')
  await page.keyboard.press('Escape')
  await expect(page.getByRole('button', { name: 'Card type: Cloze' })).toBeFocused()
  expect((await backendSnapshot(page)).dismissedQuickAdd).toBe(0)

  await page.keyboard.press('Escape')
  expect((await backendSnapshot(page)).dismissedQuickAdd).toBe(1)
})

async function backendSnapshot(page: import('@playwright/test').Page) {
  return page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
}
