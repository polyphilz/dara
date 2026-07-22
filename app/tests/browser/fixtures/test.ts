import { expect, test as base } from '@playwright/test'

const FIXED_BROWSER_TIME = new Date('2026-07-17T16:00:00-04:00')

export const test = base.extend({
  page: async ({ page }, runTest) => {
    const errors: string[] = []
    page.on('console', (message) => {
      if (message.type() === 'error') {
        errors.push(`console: ${message.text()}`)
      }
    })
    page.on('pageerror', (error) => errors.push(`pageerror: ${error.message}`))
    page.on('requestfailed', (request) => {
      errors.push(`requestfailed: ${request.url()} ${request.failure()?.errorText ?? ''}`)
    })
    await page.route('**/*', async (route) => {
      const url = new URL(route.request().url())
      if (url.hostname !== '127.0.0.1') {
        errors.push(`external request: ${url.href}`)
        await route.abort('blockedbyclient')
        return
      }
      await route.continue()
    })
    await page.clock.install({ time: FIXED_BROWSER_TIME })
    await page.emulateMedia({ colorScheme: 'light', reducedMotion: 'reduce' })
    await runTest(page)
    expect(errors, 'unexpected browser errors').toEqual([])
  },
})

export { expect } from '@playwright/test'
