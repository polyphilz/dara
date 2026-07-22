import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

const runPropertyTests = process.env.DARA_PROPERTY_SEED !== undefined

export default defineConfig({
  plugins: [react()],
  test: {
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json-summary', 'html'],
      reportsDirectory: 'coverage/frontend',
      thresholds: {
        'src/review/keyboard.ts': {
          branches: 100,
          functions: 100,
          lines: 100,
          statements: 100,
        },
      },
    },
    environment: 'jsdom',
    exclude: runPropertyTests ? [] : ['tests/ui/properties/**'],
    include: runPropertyTests
      ? ['tests/ui/properties/**/*.test.{ts,tsx}']
      : ['tests/ui/**/*.test.{ts,tsx}'],
    setupFiles: ['./tests/setup.ts'],
  },
})
