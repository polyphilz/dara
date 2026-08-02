import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import type { Locator, Page } from '@playwright/test'
import { expect, test } from '../fixtures/test.ts'

/*
 * The type scale is expressed in rem so the whole interface survives a user
 * text-size increase. These checks enlarge the root and apply the WCAG text
 * spacing overrides, then confirm that copy and actions remain present and
 * operable. Reflow and vertical scrolling are acceptable; losing content,
 * clipping actions, or scrolling the page sideways is not.
 */

const ENLARGED_ROOT_FONT_SIZE = '32px'

const WCAG_TEXT_SPACING = `
  * {
    line-height: 1.5 !important;
    letter-spacing: 0.12em !important;
    word-spacing: 0.16em !important;
  }

  p {
    margin-bottom: 2em !important;
  }
`

async function enlargeRootText(page: Page) {
  await page.evaluate((size) => {
    document.documentElement.style.fontSize = size
  }, ENLARGED_ROOT_FONT_SIZE)
}

async function applyTextSpacingOverrides(page: Page) {
  await page.addStyleTag({ content: WCAG_TEXT_SPACING })
}

async function expectNoHorizontalPageScroll(page: Page) {
  const overflow = await page.evaluate(() => {
    const root = document.documentElement
    return root.scrollWidth - root.clientWidth
  })
  expect(overflow).toBeLessThanOrEqual(1)
}

async function expectOperable(scope: Page | Locator, name: string | RegExp) {
  const control = scope.getByRole('button', { name }).first()
  await expect(control).toBeVisible()
  await expect(control).toBeEnabled()
  const box = await control.boundingBox()
  expect(box?.width ?? 0).toBeGreaterThan(0)
  expect(box?.height ?? 0).toBeGreaterThan(0)
}

async function openSettings(page: Page) {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: 'Settings' }).click()
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeFocused()
}

test('the design system catalog survives 200% text enlargement', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await enlargeRootText(page)

  await expect(page.getByRole('heading', { name: 'Dara design system' })).toBeVisible()
  await expect(
    page.getByText('This device does not have any Dara data yet.'),
  ).toBeVisible()
  await expect(page.getByLabel('Bucket name')).toBeVisible()
  await expectOperable(page, 'SURFACE')
  await expectNoHorizontalPageScroll(page)
})

test('Settings survives 200% text enlargement', async ({ page }) => {
  await openSettings(page)
  await enlargeRootText(page)

  await expect(page.getByRole('heading', { name: 'General' })).toBeVisible()
  await expect(
    page.getByText('Keep Dara available in the menu bar after you sign in.'),
  ).toBeVisible()
  await expect(page.getByRole('switch', { name: 'Launch at login' })).toBeVisible()
  await expectOperable(page, 'Reset shortcuts')
  await expectNoHorizontalPageScroll(page)
})

test('a confirmation dialog survives 200% text enlargement', async ({ page }) => {
  await openSettings(page)
  await page.getByRole('button', { name: 'Repair data' }).click()
  const dialog = page.getByRole('alertdialog', { name: 'Repair scheduling data?' })
  await expect(dialog).toBeVisible()
  await enlargeRootText(page)

  await expect(dialog.getByText('Confirm change')).toBeVisible()
  await expect(
    dialog.getByText(/Dara will replace only scheduling caches/),
  ).toBeVisible()
  await expectOperable(dialog, 'Repair data')
  await expectOperable(dialog, 'Cancel')
  await expectNoHorizontalPageScroll(page)
})

test('Settings survives the WCAG text-spacing overrides', async ({ page }) => {
  await openSettings(page)
  await applyTextSpacingOverrides(page)

  await expect(page.getByRole('heading', { name: 'Review' })).toBeVisible()
  await expect(
    page.getByText(/Higher values mean more reviews and less forgetting/),
  ).toBeVisible()
  await expect(
    page.getByRole('slider', { name: 'Desired retention' }),
  ).toBeVisible()
  await expectOperable(page, 'Reset shortcuts')
  await expectNoHorizontalPageScroll(page)
})

test('the design system catalog survives the WCAG text-spacing overrides', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  await applyTextSpacingOverrides(page)

  await expect(page.getByRole('heading', { name: 'Dara design system' })).toBeVisible()
  await expect(page.getByText('Checkpoint 4 of 12 · 1.2 MB')).toBeVisible()
  await expectOperable(page, 'SURFACE')
  await expectNoHorizontalPageScroll(page)
})

test('the recovery welcome surface survives 200% text enlargement', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.Recovery}`,
  )
  const choices = page.getByRole('button', { name: /Start fresh/ })
  await expect(choices).toBeVisible()
  await enlargeRootText(page)

  await expect(
    page.getByRole('heading', { level: 1, name: 'How would you like to begin?' }),
  ).toBeVisible()
  await expect(
    page.getByText('This device does not have any Dara data yet.'),
  ).toBeVisible()
  await expectOperable(page, /Start fresh/)
  await expectOperable(page, /Restore from backup/)
  await expectNoHorizontalPageScroll(page)
})

test('the recovery R2 form survives 200% text enlargement', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.Recovery}`,
  )
  await page.getByRole('button', { name: /Restore from backup/ }).click()
  await expect(page.getByLabel('R2 account ID')).toBeVisible()
  await enlargeRootText(page)

  for (const field of ['R2 account ID', 'Bucket', 'Access Key ID', 'Secret Access Key']) {
    await expect(page.getByLabel(field)).toBeVisible()
  }
  await expectOperable(page, 'Find backups')
  await expectOperable(page, 'Back')
  await expectNoHorizontalPageScroll(page)
})

test('the recovery welcome surface survives the WCAG text-spacing overrides', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.Recovery}`,
  )
  await expect(page.getByRole('button', { name: /Start fresh/ })).toBeVisible()
  await applyTextSpacingOverrides(page)

  await expect(
    page.getByText('This device does not have any Dara data yet.'),
  ).toBeVisible()
  await expectOperable(page, /Start fresh/)
  await expectOperable(page, /Restore from backup/)
  await expectNoHorizontalPageScroll(page)
})
