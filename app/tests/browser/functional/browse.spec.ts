import type { Page } from '@playwright/test'
import { DaraIpcCommand } from '../../../src/lib/tauri-contracts.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Browse waits for Enter to search and clearing immediately restores browse', async ({ page }) => {
  await openMain(page)
  await page.getByRole('button', { name: 'Browse' }).click()
  const search = page.getByRole('searchbox', { name: 'Search cards' })
  await expect(search).toBeVisible()
  await expect(page.getByText('All cards')).toBeVisible()
  const initialCount = (await searchCommands(page)).length

  await search.fill('retrieval practice')
  await expect(page.getByText('Press Enter to search')).toBeVisible()
  expect(await searchCommands(page)).toHaveLength(initialCount)

  await search.press('Enter')
  await expect.poll(async () => (await searchCommands(page)).length).toBe(initialCount + 1)
  expect((await searchCommands(page)).at(-1)?.payload).toMatchObject({
    input: { query: 'retrieval practice', offset: 0 },
  })
  await expect(page.getByText('Hybrid matches')).toBeVisible()

  await search.fill('')
  await expect.poll(async () => (await searchCommands(page)).length).toBe(initialCount + 2)
  expect((await searchCommands(page)).at(-1)?.payload).toMatchObject({
    input: { query: '', offset: 0 },
  })
  await expect(page.getByText('All cards')).toBeVisible()
})

test('Meta+F focuses and selects the Browse query', async ({ page }) => {
  await openMain(page)
  await page.getByRole('button', { name: 'Browse' }).click()
  const search = page.getByRole('searchbox', { name: 'Search cards' })
  await search.fill('selected query')
  await page.getByRole('button', { name: 'Home' }).focus()

  await page.keyboard.press('Meta+f')
  await expect(search).toBeFocused()
  expect(await search.evaluate((input) => ({
    end: (input as HTMLInputElement).selectionEnd,
    start: (input as HTMLInputElement).selectionStart,
  }))).toEqual({ end: 14, start: 0 })
})

test('Browse edits, pauses, resumes, and deletes a seeded card', async ({ page }) => {
  await openMain(page, BrowserScenarioId.MainBrowseBasic)
  await page.getByRole('button', { name: 'Browse' }).click()

  const result = page.getByRole('option', {
    name: /Why does retrieval practice work\?/,
  })
  await expect(result).toBeVisible()
  await expect(
    page.getByText('It strengthens the route used to recall the memory.'),
  ).toBeVisible()

  await page.getByRole('button', { name: /Edit/ }).click()
  const front = page.getByRole('textbox', { name: 'Front' })
  await expect(front).toBeFocused()
  await front.fill('Why is retrieval practice durable?')
  await page.getByRole('button', { name: /Save/ }).click()
  await expect(
    page.getByRole('option', { name: /Why is retrieval practice durable\?/ }),
  ).toBeVisible()

  await page.getByRole('button', { name: /Pause/ }).click()
  await expect(page.getByRole('button', { name: /Resume/ })).toBeVisible()
  await page.getByRole('button', { name: /Resume/ }).click()
  await expect(page.getByRole('button', { name: /Pause/ })).toBeVisible()

  await page.getByRole('button', { name: 'Delete' }).click()
  await expect(page.getByRole('alert')).toContainText('Delete this card?')
  await page.getByRole('button', { name: 'Delete card' }).click()
  await expect(page.getByText('No cards found.')).toBeVisible()

  const commandNames = (await backendSnapshot(page)).commands.map(
    ({ command }) => command,
  )
  expect(commandNames).toContain(DaraIpcCommand.UpdateCardContent)
  expect(
    commandNames.filter(
      (command) => command === DaraIpcCommand.SetCardContentSuspended,
    ),
  ).toHaveLength(2)
  expect(commandNames).toContain(DaraIpcCommand.DeleteCardContent)
})

async function openMain(
  page: Page,
  scenario: (typeof BrowserScenarioId)[keyof typeof BrowserScenarioId] =
    BrowserScenarioId.MainReviewBasic,
) {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${scenario}&surface=${BrowserHarnessSurface.Main}`,
  )
}

async function searchCommands(page: Page) {
  const snapshot = await backendSnapshot(page)
  return snapshot.commands.filter(
    ({ command }) => command === DaraIpcCommand.SearchCardContent,
  )
}

async function backendSnapshot(page: Page) {
  return page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
}
