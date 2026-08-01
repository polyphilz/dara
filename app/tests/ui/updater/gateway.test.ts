import { beforeEach, expect, test, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  close: vi.fn(),
  downloadAndInstall: vi.fn(),
  relaunch: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mocks.check,
}))

vi.mock('@tauri-apps/plugin-process', () => ({
  relaunch: mocks.relaunch,
}))

beforeEach(() => {
  vi.clearAllMocks()
  mocks.check.mockResolvedValue({
    body: null,
    close: mocks.close,
    currentVersion: '0.1.0',
    date: null,
    downloadAndInstall: mocks.downloadAndInstall,
    version: '0.2.0',
  })
  mocks.downloadAndInstall.mockResolvedValue(undefined)
})

test('bounds update checks and downloads with explicit timeouts', async () => {
  const { tauriUpdateGateway } = await import(
    '../../../src/updater/gateway.ts'
  )

  await tauriUpdateGateway.check()
  expect(mocks.check).toHaveBeenCalledWith({ timeout: 30_000 })

  await tauriUpdateGateway.downloadAndInstall(vi.fn())
  expect(mocks.downloadAndInstall).toHaveBeenCalledWith(
    expect.any(Function),
    { timeout: 10 * 60 * 1_000 },
  )
})
