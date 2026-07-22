import type { Page } from '@playwright/test'
import { DaraEvent } from '../../../src/lib/tauri-contracts.ts'
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

async function openMain(page: Page) {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
}
