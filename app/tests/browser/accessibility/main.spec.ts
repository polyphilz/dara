import AxeBuilder from '@axe-core/playwright'
import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Home and revealed Review have no detectable accessibility violations', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await expect(page.getByRole('region', { name: 'Review activity' })).toBeVisible()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])

  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(page.getByRole('button', { name: 'Reveal answer' })).toBeVisible()
  await page.keyboard.press('Space')
  await expect(page.getByRole('group', { name: 'Grade this card' })).toBeVisible()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])
})

test('Cloze Review question has no detectable accessibility violations', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewCloze}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: /Review.*reviewed today/ }).click()
  await expect(
    page.getByRole('note', { name: 'Hidden cloze deletion' }),
  ).toHaveCount(2)

  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])
})

test('Browse and Settings have no detectable accessibility violations', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 720 })
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )

  await page.getByRole('button', { name: 'Browse' }).click()
  await expect(page.getByRole('searchbox', { name: 'Search cards' })).toBeVisible()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])

  await page.getByRole('button', { name: 'Settings' }).click()
  await expect(
    page.getByRole('heading', { name: 'Settings', exact: true }),
  ).toBeFocused()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])
})
