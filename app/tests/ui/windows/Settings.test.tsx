import { fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import { DEFAULT_SCHEDULER_CONFIG } from '../../../src/scheduling/config.ts'
import type {
  InstallSchedulerReplayInput,
  SchedulerConfigRecord,
  SchedulerMaintenanceGateway,
  SchedulerReplayInstallReport,
  SchedulerReplaySnapshot,
} from '../../../src/scheduling/index.ts'
import {
  Appearance,
  DaraCommand,
  type SettingsGateway,
  type SettingsSnapshot,
} from '../../../src/settings/index.ts'
import { Settings } from '../../../src/windows/main/Settings.tsx'

const eventMocks = vi.hoisted(() => ({ listen: vi.fn() }))

vi.mock('@tauri-apps/api/event', () => ({ listen: eventMocks.listen }))

const ACTIVE_CONFIG_ID = '019f547b-6200-7000-8000-000000000001'
const TARGET_CONFIG_ID = '019f547b-6200-7000-8000-000000000002'

beforeEach(() => {
  vi.clearAllMocks()
  eventMocks.listen.mockResolvedValue(() => undefined)
})

test('stages retention, explains the recalculation, and changes nothing on cancel', async () => {
  const fixture = settingsFixture()
  const settingsGateway = fixture.gateway
  const schedulerGateway = schedulerFixture(fixture)
  const { findByRole, getByRole, queryByRole } = renderSettings(
    settingsGateway,
    schedulerGateway,
  )

  const slider = await findByRole('slider', { name: 'Desired retention' })
  expect((slider as HTMLInputElement).value).toBe('90')
  expect(queryByRole('button', { name: 'Restore 90%' })).toBeNull()
  fireEvent.change(slider, { target: { value: '85' } })

  expect(schedulerGateway.prepareDesiredRetentionReplay).not.toHaveBeenCalled()
  expect(getByRole('button', { name: 'Restore 90%' })).toBeTruthy()
  fireEvent.click(getByRole('button', { name: 'Update schedule' }))
  const dialog = getByRole('alertdialog', {
    name: 'Recalculate reviewed cards?',
  })
  expect(dialog.textContent).toContain('review history will not change')
  expect(dialog.textContent).toContain('current schedule stays active')

  fireEvent.click(getByRole('button', { name: 'Cancel' }))
  expect(queryByRole('alertdialog')).toBeNull()
  expect(schedulerGateway.prepareDesiredRetentionReplay).not.toHaveBeenCalled()
})

test('confirmed retention invokes the atomic replay workflow and refreshes the setting', async () => {
  const fixture = settingsFixture()
  const settingsGateway = fixture.gateway
  const schedulerGateway = schedulerFixture(fixture)
  const onBusyChange = vi.fn()
  const onSchedulingChanged = vi.fn()
  const { findByRole, findByText, getByRole } = render(
    <Settings
      navigationToken={1}
      onBusyChange={onBusyChange}
      onSchedulingChanged={onSchedulingChanged}
      reviewSaveInFlight={false}
      schedulerGateway={schedulerGateway}
      settingsGateway={settingsGateway}
    />,
  )

  fireEvent.change(await findByRole('slider', { name: 'Desired retention' }), {
    target: { value: '85' },
  })
  fireEvent.click(getByRole('button', { name: 'Update schedule' }))
  fireEvent.click(getByRole('button', { name: 'Update to 85%' }))

  expect(
    await findByText('Desired retention is now 85%. Recalculated 0 reviewed cards.'),
  ).toBeTruthy()
  expect(schedulerGateway.prepareDesiredRetentionReplay).toHaveBeenCalledWith(0.85)
  expect(schedulerGateway.installSchedulerReplay).toHaveBeenCalledTimes(1)
  expect(onSchedulingChanged).toHaveBeenCalledTimes(1)
  expect(onBusyChange).toHaveBeenCalledWith(true)
  expect(onBusyChange).toHaveBeenLastCalledWith(false)
  expect((getByRole('slider', { name: 'Desired retention' }) as HTMLInputElement).value).toBe('85')
})

test('failed retention keeps the proposed value unapplied and reports the error', async () => {
  const fixture = settingsFixture()
  const schedulerGateway = schedulerFixture(fixture)
  schedulerGateway.installSchedulerReplay.mockRejectedValueOnce(
    new Error('A reviewed card changed; try again.'),
  )
  const { findByRole, findByText, getByRole } = renderSettings(
    fixture.gateway,
    schedulerGateway,
  )

  const slider = await findByRole('slider', { name: 'Desired retention' })
  fireEvent.change(slider, { target: { value: '95' } })
  fireEvent.click(getByRole('button', { name: 'Update schedule' }))
  fireEvent.click(getByRole('button', { name: 'Update to 95%' }))

  expect(await findByText('A reviewed card changed; try again.')).toBeTruthy()
  expect((slider as HTMLInputElement).value).toBe('95')
  expect(getByRole('alertdialog')).toBeTruthy()
  expect(fixture.current.desiredRetention).toBe(0.9)
})

test('launch-at-login applies through the system-backed settings command', async () => {
  const fixture = settingsFixture()
  const schedulerGateway = schedulerFixture(fixture)
  const { findByRole } = renderSettings(fixture.gateway, schedulerGateway)

  fireEvent.click(await findByRole('switch', { name: 'Launch at login' }))

  await waitFor(() => {
    expect(fixture.gateway.setLaunchAtLogin).toHaveBeenCalledWith(true)
  })
})

function renderSettings(
  settingsGateway: MockSettingsGateway,
  schedulerGateway: MockSchedulerGateway,
) {
  return render(
    <Settings
      navigationToken={1}
      onBusyChange={vi.fn()}
      onSchedulingChanged={vi.fn()}
      reviewSaveInFlight={false}
      schedulerGateway={schedulerGateway}
      settingsGateway={settingsGateway}
    />,
  )
}

interface SettingsFixture {
  current: SettingsSnapshot
  gateway: MockSettingsGateway
}

type MockSettingsGateway = SettingsGateway & {
  [Key in keyof SettingsGateway]: ReturnType<typeof vi.fn>
}

type MockSchedulerGateway = SchedulerMaintenanceGateway & {
  installSchedulerReplay: ReturnType<typeof vi.fn>
  loadSchedulerReplaySnapshot: ReturnType<typeof vi.fn>
  prepareDesiredRetentionReplay: ReturnType<typeof vi.fn>
}

function settingsFixture(): SettingsFixture {
  const current: SettingsSnapshot = {
    appearance: Appearance.System,
    desiredRetention: 0.9,
    keyboardBindings: [
      {
        accelerator: 'control+alt+super+KeyD',
        command: DaraCommand.QuickAdd,
      },
      {
        accelerator: 'control+alt+super+KeyH',
        command: DaraCommand.Home,
      },
    ],
    launchAtLogin: false,
    launchAtLoginError: null,
    legacyZoomMigrated: true,
    revision: 1,
    shortcutErrors: [],
    zoomPercent: 100,
  }
  const gateway = {
    adoptLegacyZoom: vi.fn(),
    loadSettings: vi.fn(async () => structuredClone(current)),
    setAppearance: vi.fn(),
    setKeyboardBindings: vi.fn(),
    setLaunchAtLogin: vi.fn(async (enabled: boolean) => {
      current.launchAtLogin = enabled
      return structuredClone(current)
    }),
    setZoomPercent: vi.fn(),
  } as unknown as MockSettingsGateway
  return { current, gateway }
}

function schedulerFixture(fixture: SettingsFixture): MockSchedulerGateway {
  let targetRetention = fixture.current.desiredRetention
  const activeSnapshot = replaySnapshot(
    ACTIVE_CONFIG_ID,
    ACTIVE_CONFIG_ID,
    fixture.current.desiredRetention,
    false,
  )
  return {
    loadSchedulerReplaySnapshot: vi.fn(async () => structuredClone(activeSnapshot)),
    prepareDesiredRetentionReplay: vi.fn(async (desiredRetention: number) => {
      targetRetention = desiredRetention
      return replaySnapshot(
        ACTIVE_CONFIG_ID,
        TARGET_CONFIG_ID,
        desiredRetention,
        true,
      )
    }),
    installSchedulerReplay: vi.fn(
      async (
        input: InstallSchedulerReplayInput,
      ): Promise<SchedulerReplayInstallReport> => {
        fixture.current.desiredRetention = targetRetention
        return {
          activeSchedulerConfigId: input.targetSchedulerConfig.id,
          evaluatedCards: input.cards.length,
          installedCards: input.cards.length,
          operation: input.operation,
        }
      },
    ),
  } as MockSchedulerGateway
}

function replaySnapshot(
  sourceId: string,
  targetId: string,
  desiredRetention: number,
  targetIsNew: boolean,
): SchedulerReplaySnapshot {
  const config = structuredClone(DEFAULT_SCHEDULER_CONFIG)
  config.config.desiredRetention = desiredRetention
  const targetSchedulerConfig: SchedulerConfigRecord = {
    id: targetId,
    ...config,
  }
  return {
    cards: [],
    sourceActiveSchedulerConfigId: sourceId,
    targetIsNew,
    targetSchedulerConfig,
  }
}
