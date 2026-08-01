import { render } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'

const listen = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => false,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen,
}))

afterEach(() => {
  vi.clearAllMocks()
})

test('does not register native update listeners outside Tauri', async () => {
  const { AppUpdater } = await import('../../../src/updater/AppUpdater.tsx')

  render(<AppUpdater />)

  expect(listen).not.toHaveBeenCalled()
})

test('enables updates only for an explicitly marked Tauri release', async () => {
  const { updaterIsEnabled } = await import('../../../src/updater/index.ts')

  expect(updaterIsEnabled(true, 'true')).toBe(true)
  expect(updaterIsEnabled(true, undefined)).toBe(false)
  expect(updaterIsEnabled(true, 'false')).toBe(false)
  expect(updaterIsEnabled(false, 'true')).toBe(false)
})
