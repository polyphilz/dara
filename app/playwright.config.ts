import { defineConfig, devices } from '@playwright/test'

const CI_RETRIES = 1

export default defineConfig({
  testDir: './tests/browser',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? CI_RETRIES : 0,
  failOnFlakyTests: true,
  reporter: process.env.CI
    ? [['line'], ['html', { open: 'never' }]]
    : [['list'], ['html', { open: 'never' }]],
  outputDir: 'test-results/playwright',
  snapshotPathTemplate:
    '{testDir}/__screenshots__/{projectName}/{testFilePath}/{arg}{ext}',
  expect: {
    toHaveScreenshot: {
      animations: 'disabled',
      caret: 'hide',
    },
  },
  use: {
    baseURL: 'http://127.0.0.1:4173',
    colorScheme: 'light',
    locale: 'en-US',
    timezoneId: 'America/New_York',
    trace: 'retain-on-failure',
  },
  webServer: {
    command:
      'pnpm exec vite --config vite.browser.config.ts --host 127.0.0.1 --port 4173',
    url: 'http://127.0.0.1:4173/tests/browser/harness/',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [
    {
      name: 'chromium-functional',
      testMatch: [
        'functional/**/*.spec.ts',
        'accessibility/**/*.spec.ts',
      ],
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 920, height: 760 },
      },
    },
    {
      name: 'webkit-contract',
      testMatch: ['contract/**/*.spec.ts'],
      use: {
        ...devices['Desktop Safari'],
        viewport: { width: 920, height: 760 },
      },
    },
    {
      name: 'chromium-visual',
      testMatch: ['visual/**/*.spec.ts'],
      workers: 1,
      use: {
        ...devices['Desktop Chrome'],
        deviceScaleFactor: 1,
        viewport: { width: 920, height: 760 },
      },
    },
    {
      name: 'chromium-hidpi-visual',
      testMatch: ['visual-hidpi/**/*.spec.ts'],
      workers: 1,
      use: {
        ...devices['Desktop Chrome HiDPI'],
        deviceScaleFactor: 2,
        viewport: { width: 920, height: 760 },
      },
    },
  ],
})
