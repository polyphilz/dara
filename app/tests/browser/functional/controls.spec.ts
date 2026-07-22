import { BrowserHarnessSurface, BrowserScenarioId } from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

test('shared listbox supports keyboard open, selection, Escape, and focus return', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  const trigger = page.getByRole('button', { name: 'Catalog choice: Second option' })
  await trigger.focus()
  await page.keyboard.press('Enter')
  const listbox = page.getByRole('listbox', { name: 'Catalog choice' })
  await expect(listbox).toBeVisible()
  await expect(page.getByRole('option', { name: 'Second option' })).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: 'Catalog choice: Third option' })).toBeVisible()

  await page.getByRole('button', { name: 'Catalog choice: Third option' }).press('Enter')
  await page.keyboard.press('Escape')
  await expect(listbox).toBeHidden()
  await expect(page.getByRole('button', { name: 'Catalog choice: Third option' })).toBeFocused()
})
