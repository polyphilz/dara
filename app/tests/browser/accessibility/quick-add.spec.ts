import AxeBuilder from '@axe-core/playwright'
import { BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Quick Add Basic has no detectable accessibility violations', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  await expect(page.getByRole('region', { name: 'Quick add' })).toBeVisible()
  const results = await new AxeBuilder({ page }).analyze()
  expect(results.violations).toEqual([])
})

test('Quick Add Cloze, Occlusion, and open card-type listbox are accessible', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}`,
  )
  const trigger = page.getByRole('button', { name: 'Card type: Basic' })
  await expect(page.getByRole('textbox', { name: 'Front' })).toBeFocused()
  await trigger.focus()
  await trigger.press('Enter')
  await expect(page.getByRole('option', { name: 'Basic' })).toBeFocused()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])

  await page.getByRole('option', { name: 'Cloze' }).click()
  await expect(page.getByRole('textbox', { name: 'Text' })).toBeVisible()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])

  await page.getByRole('button', { name: 'Card type: Cloze' }).click()
  await page.getByRole('option', { name: 'Image occlusion' }).click()
  await expect(
    page.getByRole('button', { name: 'Choose an image for occlusion' }),
  ).toBeVisible()
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([])
})
