import type { Page } from '@playwright/test'
import { DaraEvent } from '../../../src/lib/tauri-contracts.ts'
import { Appearance } from '../../../src/settings/types.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('main navigation order is stable and every Settings route focuses its heading', async ({ page }) => {
  await openMain(page)
  await expect(page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button'))
    .toHaveText(['Home', 'Add', 'Browse', 'Settings'])

  await page.getByRole('button', { name: 'Settings' }).click()
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeFocused()
  await expect(page.getByText('Dara 0.1.0', { exact: true })).toBeVisible()
  await expect(page.getByText('Database main 7 · media 4')).toBeVisible()
  await expect(page.getByText(/Jina Embeddings v5 Text Nano/)).toBeVisible()

  await page.getByRole('button', { name: 'Home', exact: true }).click()
  await page.keyboard.press('Meta+,')
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeFocused()

  await page.getByRole('button', { name: 'Home', exact: true }).click()
  await page.evaluate(async (event) => {
    await (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.emit(event)
  }, DaraEvent.OpenSettings)
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeFocused()
})

test('off-site backup clears credentials and distinguishes syncing from recoverability', async ({
  page,
}) => {
  await openMain(page)
  await page.getByRole('button', { name: 'Settings' }).click()
  await expect(
    page.getByRole('button', { name: 'Test and enable backup' }),
  ).toBeVisible()

  const accessKey = '11111111111111111111111111111111'
  const secretKey =
    '2222222222222222222222222222222222222222222222222222222222222222'
  await page
    .getByLabel('Account ID')
    .fill('0123456789abcdef0123456789abcdef')
  await page.getByLabel('Bucket').fill('dara-local')
  await page.getByLabel('Access Key ID').fill(accessKey)
  await page.getByLabel('Secret Access Key').fill(secretKey)
  await page.getByRole('button', { name: 'Test and enable backup' }).click()

  await expect(page.getByText('Running', { exact: true })).toBeVisible()
  await expect(page.getByText('Not ready', { exact: true })).toBeVisible()
  await expect(
    page.getByText(/no complete recoverable checkpoint yet/i),
  ).toBeVisible()
  await expect(page.getByLabel('Access Key ID')).toHaveCount(0)
  const snapshot = await page.evaluate(
    () =>
      (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
        .__DARA_BROWSER_TEST__.snapshot(),
  )
  const recorded = JSON.stringify(snapshot.commands)
  expect(recorded).not.toContain(accessKey)
  expect(recorded).not.toContain(secretKey)
  expect(recorded).toContain('[REDACTED]')

  await page.getByRole('button', { name: 'Back up now' }).click()
  await expect(page.getByText(/checkpoint 019f547b/)).toBeVisible()
  await page.getByRole('button', { name: 'Run restore drill' }).click()
  await expect(page.getByText('Passed', { exact: true })).toBeVisible()
})

test('off-site backup controls remain usable in dark appearance', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}&appearance=${Appearance.Dark}`,
  )
  await page.getByRole('button', { name: 'Settings' }).click()

  await expect(page.locator('html')).toHaveAttribute(
    'data-appearance',
    Appearance.Dark,
  )
  await expect(page.getByLabel('R2 jurisdiction')).toBeVisible()
  await expect(
    page.getByRole('button', { name: 'Test and enable backup' }),
  ).toBeVisible()
})

async function openMain(page: Page) {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
}
