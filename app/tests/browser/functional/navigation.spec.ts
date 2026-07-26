import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('Add protects a dirty draft while explicit Cancel discards it', async ({
  page,
}) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: 'Add' }).click()
  const front = page.getByRole('textbox', { name: 'Front' })
  await front.fill('unfinished add draft')

  await page.getByRole('button', { name: 'Home' }).click()
  const dialog = page.getByRole('alertdialog', {
    name: 'Discard unsaved changes?',
  })
  await expect(dialog).toBeVisible()
  await expect(front).toHaveText('unfinished add draft')

  await dialog.getByRole('button', { name: 'Keep editing' }).click()
  await expect(dialog).toBeHidden()
  await expect(front).toBeFocused()

  await page.getByRole('button', { name: 'Cancel' }).click()
  await expect(page.getByRole('region', { name: 'Review activity' })).toBeVisible()
  await expect(dialog).toBeHidden()
})
