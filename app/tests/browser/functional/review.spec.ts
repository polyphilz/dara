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

async function snapshot(page: Page) {
  return page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
}
