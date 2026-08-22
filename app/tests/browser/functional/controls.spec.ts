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
  const selectedOption = page.getByRole('option', { name: 'Second option' })
  const nextOption = page.getByRole('option', { name: 'Third option' })
  await expect(selectedOption).toBeFocused()
  const selectedHighlight = await selectedOption.evaluate(
    (option) => getComputedStyle(option).backgroundColor,
  )
  await page.keyboard.press('ArrowDown')
  await expect(nextOption).toBeFocused()
  await expect
    .poll(() =>
      nextOption.evaluate(
        (option) => getComputedStyle(option).backgroundColor,
      ),
    )
    .toBe(selectedHighlight)
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: 'Catalog choice: Third option' })).toBeVisible()

  await page.getByRole('button', { name: 'Catalog choice: Third option' }).press('Enter')
  await page.keyboard.press('Escape')
  await expect(listbox).toBeHidden()
  await expect(page.getByRole('button', { name: 'Catalog choice: Third option' })).toBeFocused()
})

test('shared listbox stays open after a complete held pointer click', async ({ page }) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.QuickAddEmpty}&surface=${BrowserHarnessSurface.VisualCatalog}`,
  )
  const trigger = page.getByRole('button', {
    name: 'Catalog choice: Second option',
  })
  const listbox = page.getByRole('listbox', { name: 'Catalog choice' })

  await trigger.click({ delay: 75 })
  await expect(listbox).toBeVisible()
  await expect(trigger).toHaveAttribute('aria-expanded', 'true')

  await trigger.click({ delay: 75 })
  await expect(listbox).toBeHidden()
  await expect(trigger).toHaveAttribute('aria-expanded', 'false')
})
