import type { Page } from '@playwright/test'
import { DaraIpcCommand } from '../../../src/lib/tauri-contracts.ts'
import { MainWindowRoutePath } from '../../../src/windows/main/main-window-routes.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

const SEEDED_CARD_CONTENT_ID =
  '01980c8e-6c00-7000-8000-000000000001'

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

test('Browse edit uses Back and Forward without losing Browse state or focus', async ({
  page,
}) => {
  await openMain(page, BrowserScenarioId.MainBrowseBasic)
  await page.getByRole('button', { name: 'Browse' }).click()
  const search = page.getByRole('searchbox', { name: 'Search cards' })
  await search.fill('retrieval')
  await search.press('Enter')
  const result = page.getByRole('option', {
    name: /Why does retrieval practice work\?/,
  })
  await expect(result).toBeVisible()

  await page.getByRole('button', { name: /Edit/ }).click()
  const front = page.getByRole('textbox', { name: 'Front' })
  await expect(front).toBeFocused()
  await expect(page).toHaveURL(
    new RegExp(`#${editPath(SEEDED_CARD_CONTENT_ID)}$`),
  )

  await page.goBack()
  await expect(page).toHaveURL(
    new RegExp(`#${browseCardPath(SEEDED_CARD_CONTENT_ID)}$`),
  )
  await expect(search).toHaveValue('retrieval')
  await expect(result).toBeFocused()

  await page.goForward()
  await expect(page).toHaveURL(
    new RegExp(`#${editPath(SEEDED_CARD_CONTENT_ID)}$`),
  )
  await expect(front).toBeFocused()
})

test('Browse Back and Forward traverse deliberate card selections', async ({
  page,
}) => {
  await openMain(page, BrowserScenarioId.MainBrowseHistory)
  await page.getByRole('button', { name: 'Browse' }).click()

  await expect(page).toHaveURL(
    new RegExp(`#${browseCardPath(cardContentId(5))}$`),
  )
  for (const number of ['one', 'two', 'three', 'four']) {
    await page
      .getByRole('option', { name: new RegExp(`History card ${number}`) })
      .click()
  }
  await expect(page).toHaveURL(
    new RegExp(`#${browseCardPath(cardContentId(4))}$`),
  )

  await page.goBack()
  await expect(page).toHaveURL(
    new RegExp(`#${browseCardPath(cardContentId(3))}$`),
  )
  await expect(
    page.getByRole('option', { name: /History card three/ }),
  ).toHaveAttribute('aria-selected', 'true')
  await expect(page.getByText('History answer three')).toBeVisible()

  await page.goBack()
  await expect(
    page.getByRole('option', { name: /History card two/ }),
  ).toHaveAttribute('aria-selected', 'true')

  await page.goForward()
  await expect(
    page.getByRole('option', { name: /History card three/ }),
  ).toHaveAttribute('aria-selected', 'true')
})

test('dirty editors require an app-owned discard decision', async ({ page }) => {
  await openMain(page, BrowserScenarioId.MainBrowseBasic)
  await page.getByRole('button', { name: 'Browse' }).click()
  await page.getByRole('button', { name: /Edit/ }).click()
  const front = page.getByRole('textbox', { name: 'Front' })
  await expect(front).toBeFocused()
  await front.fill('unfinished route-aware draft')

  await page.getByRole('button', { name: 'Home' }).click()
  const dialog = page.getByRole('alertdialog', {
    name: 'Discard unsaved changes?',
  })
  await expect(dialog).toBeVisible()
  await expect(page).toHaveURL(
    new RegExp(`#${editPath(SEEDED_CARD_CONTENT_ID)}$`),
  )
  await expect(front).toHaveText('unfinished route-aware draft')
  await expect(
    dialog.getByRole('button', { name: 'Keep editing' }),
  ).toBeFocused()

  await dialog.getByRole('button', { name: 'Keep editing' }).click()
  await expect(dialog).toBeHidden()
  await expect(front).toBeFocused()

  await page.getByRole('button', { name: 'Home' }).click()
  await page
    .getByRole('alertdialog', { name: 'Discard unsaved changes?' })
    .getByRole('button', { name: 'Discard changes' })
    .click()
  await expect(page.getByRole('region', { name: 'Review activity' })).toBeVisible()
  await expect(page).toHaveURL(new RegExp(`#${MainWindowRoutePath.Home}$`))
})

test('an edit URL loads its card directly by ID', async ({ page }) => {
  await openMain(
    page,
    BrowserScenarioId.MainBrowseBasic,
    editPath(SEEDED_CARD_CONTENT_ID),
  )

  await expect(page.getByRole('heading', { name: 'Edit card' })).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'Front' })).toHaveText(
    'Why does retrieval practice work?',
  )
  const loadCommands = (await backendSnapshot(page)).commands.filter(
    ({ command }) => command === DaraIpcCommand.LoadCardContent,
  )
  expect(loadCommands.length).toBeGreaterThan(0)
  expect(loadCommands).toEqual(
    loadCommands.map(() => ({
      command: DaraIpcCommand.LoadCardContent,
      payload: { cardContentId: SEEDED_CARD_CONTENT_ID },
    })),
  )
})

test('a Browse URL preserves and loads a card beyond the first result page', async ({
  page,
}) => {
  const addressedId = cardContentId(1)
  await openMain(
    page,
    BrowserScenarioId.MainBrowseDeepRoute,
    browseCardPath(addressedId),
  )

  await expect(page).toHaveURL(
    new RegExp(`#${browseCardPath(addressedId)}$`),
  )
  await expect(page.getByText('Deep route answer 1')).toBeVisible()
  await expect(page.getByRole('option')).toHaveCount(50)
  await expect(
    page.getByRole('option', { name: /^Deep route card 1\b/ }),
  ).toHaveCount(0)
  expect(
    (await backendSnapshot(page)).commands.some(
      ({ command, payload }) =>
        command === DaraIpcCommand.LoadCardContent &&
        JSON.stringify(payload) ===
          JSON.stringify({ cardContentId: addressedId }),
    ),
  ).toBe(true)
})

test('an edit URL preserves a card beyond the first result page', async ({
  page,
}) => {
  const addressedId = cardContentId(1)
  await openMain(
    page,
    BrowserScenarioId.MainBrowseDeepRoute,
    editPath(addressedId),
  )

  await expect(page).toHaveURL(
    new RegExp(`#${editPath(addressedId)}$`),
  )
  await expect(page.getByRole('textbox', { name: 'Front' })).toHaveText(
    'Deep route card 1',
  )
})

async function openMain(
  page: Page,
  scenario: (typeof BrowserScenarioId)[keyof typeof BrowserScenarioId] =
    BrowserScenarioId.MainReviewBasic,
  routePath?: string,
) {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${scenario}&surface=${BrowserHarnessSurface.Main}${routePath ? `#${routePath}` : ''}`,
  )
}

function editPath(cardContentId: string): string {
  return MainWindowRoutePath.BrowseEdit.replace(
    '$cardContentId',
    cardContentId,
  )
}

function browseCardPath(cardContentId: string): string {
  return MainWindowRoutePath.BrowseCard.replace(
    '$cardContentId',
    cardContentId,
  )
}

function cardContentId(sequence: number): string {
  return `01980c8e-6c00-7000-8000-${sequence.toString(16).padStart(12, '0')}`
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
