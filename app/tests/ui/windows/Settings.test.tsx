import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import type {
  DiagnosticsGateway,
  DiagnosticsSnapshot,
} from '../../../src/diagnostics/index.ts'
import {
  CheckpointBackupPhase,
  CredentialAvailability,
  MediaBackupPhase,
  OffsiteBackupOperationKind,
  R2Jurisdiction,
  RelationalBackupPhase,
  type OffsiteBackupGateway,
  type OffsiteBackupOperation,
  type OffsiteBackupStatus,
} from '../../../src/backup/index.ts'
import { SemanticSearchPhase } from '../../../src/review/index.ts'
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
      backupGateway={backupGatewayFixture()}
      navigationToken={1}
      onBusyChange={onBusyChange}
      onSchedulingChanged={onSchedulingChanged}
      reviewSaveInFlight={false}
      diagnosticsGateway={diagnosticsFixture()}
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

test('disables sibling settings for the full backup operation', async () => {
  const fixture = settingsFixture()
  const schedulerGateway = schedulerFixture(fixture)
  const backupGateway = backupGatewayFixture(enabledBackupStatus())
  const onBusyChange = vi.fn()
  let resolveBackup!: (operation: OffsiteBackupOperation) => void
  backupGateway.backupNow.mockImplementationOnce(
    () =>
      new Promise<OffsiteBackupOperation>((resolve) => {
        resolveBackup = resolve
      }),
  )
  const { findByRole, getByRole } = render(
    <Settings
      backupGateway={backupGateway}
      diagnosticsGateway={diagnosticsFixture()}
      navigationToken={1}
      onBusyChange={onBusyChange}
      onSchedulingChanged={vi.fn()}
      reviewSaveInFlight={false}
      schedulerGateway={schedulerGateway}
      settingsGateway={fixture.gateway}
    />,
  )

  fireEvent.click(await findByRole('button', { name: 'Back up now' }))

  await waitFor(() => {
    expect(onBusyChange).toHaveBeenLastCalledWith(true)
  })
  const schedulingCheck = getByRole('button', { name: 'Check' })
  expect((schedulingCheck as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(schedulingCheck)
  expect(schedulerGateway.loadSchedulerReplaySnapshot).not.toHaveBeenCalled()

  await act(async () => {
    resolveBackup(backupOperation())
  })

  await waitFor(() => {
    expect(onBusyChange).toHaveBeenLastCalledWith(false)
    expect((schedulingCheck as HTMLButtonElement).disabled).toBe(false)
  })
})

test('shows the cheap diagnostics snapshot without blocking settings', async () => {
  const fixture = settingsFixture()
  const diagnosticsGateway = diagnosticsFixture()
  const { findByText, getByText } = render(
    <Settings
      backupGateway={backupGatewayFixture()}
      diagnosticsGateway={diagnosticsGateway}
      navigationToken={1}
      onBusyChange={vi.fn()}
      onSchedulingChanged={vi.fn()}
      reviewSaveInFlight={false}
      schedulerGateway={schedulerFixture(fixture)}
      settingsGateway={fixture.gateway}
    />,
  )

  expect(await findByText('Dara 0.1.0')).toBeTruthy()
  expect(getByText('Database main 7 · media 4')).toBeTruthy()
  expect(getByText(/Jina Embeddings v5 Text Nano/)).toBeTruthy()
  expect(getByText(/12 of 12 cards indexed/)).toBeTruthy()
  expect(getByText('No missing referenced media found')).toBeTruthy()
  expect(diagnosticsGateway.loadDiagnostics).toHaveBeenCalledTimes(1)
})

test('keeps settings usable when diagnostics fail and retries independently', async () => {
  const fixture = settingsFixture()
  const diagnosticsGateway = diagnosticsFixture()
  diagnosticsGateway.loadDiagnostics.mockRejectedValueOnce(
    new Error('Diagnostics are temporarily unavailable.'),
  )
  const { findByRole, findByText, getByRole } = render(
    <Settings
      backupGateway={backupGatewayFixture()}
      diagnosticsGateway={diagnosticsGateway}
      navigationToken={1}
      onBusyChange={vi.fn()}
      onSchedulingChanged={vi.fn()}
      reviewSaveInFlight={false}
      schedulerGateway={schedulerFixture(fixture)}
      settingsGateway={fixture.gateway}
    />,
  )

  expect(
    await findByText('Diagnostics are temporarily unavailable.'),
  ).toBeTruthy()
  expect(await findByRole('switch', { name: 'Launch at login' })).toBeTruthy()

  fireEvent.click(getByRole('button', { name: 'Try again' }))

  expect(await findByText('Dara 0.1.0')).toBeTruthy()
  expect(diagnosticsGateway.loadDiagnostics).toHaveBeenCalledTimes(2)
})

function renderSettings(
  settingsGateway: MockSettingsGateway,
  schedulerGateway: MockSchedulerGateway,
) {
  return render(
    <Settings
      backupGateway={backupGatewayFixture()}
      navigationToken={1}
      onBusyChange={vi.fn()}
      onSchedulingChanged={vi.fn()}
      reviewSaveInFlight={false}
      diagnosticsGateway={diagnosticsFixture()}
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

type MockDiagnosticsGateway = DiagnosticsGateway & {
  loadDiagnostics: ReturnType<typeof vi.fn>
}

type MockBackupGateway = OffsiteBackupGateway & {
  [Key in keyof OffsiteBackupGateway]: ReturnType<typeof vi.fn>
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

function diagnosticsFixture(): MockDiagnosticsGateway {
  const snapshot: DiagnosticsSnapshot = {
    applicationVersion: '0.1.0',
    database: {
      migrationHeads: { main: 7, media: 4 },
      scheduler: {
        algorithm: DEFAULT_SCHEDULER_CONFIG.algorithm,
        algorithmVersion: DEFAULT_SCHEDULER_CONFIG.algorithmVersion,
        schedulerLibrary: DEFAULT_SCHEDULER_CONFIG.schedulerLibrary,
        libraryVersion: DEFAULT_SCHEDULER_CONFIG.libraryVersion,
        desiredRetention: 0.9,
      },
      semanticIndex: {
        id: 'jina_v1',
        active: true,
        indexedDocuments: 12,
        totalDocuments: 12,
      },
    },
    semanticModel: {
      downloadedBytes: 232_883_776,
      expectedBytes: 232_883_776,
      lastError: null,
      modelName: 'jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF',
      phase: SemanticSearchPhase.Ready,
    },
    storage: {
      relationalDatabaseBytes: 1_048_576,
      mediaDatabaseBytes: 524_288,
      modelBytes: 232_883_776,
      snapshotsBytes: 1_572_864,
      logsBytes: 4_096,
    },
    latestSnapshot: {
      applicationVersion: '0.1.0',
      createdAt: 1_788_512_400_000,
    },
    lastMediaMaintenance: {
      cleanup: {
        deletedBlobCount: 0,
        reclaimedBytes: 0,
        retiredImageCount: 0,
      },
      inspectedAt: 1_788_512_400_000,
      integrity: {
        extraBlobBytes: 0,
        extraBlobSha256: [],
        missingReferencedBlobImageIds: [],
        orphanedImageIds: [],
      },
    },
  }
  return {
    loadDiagnostics: vi.fn(async () => structuredClone(snapshot)),
  }
}

function backupGatewayFixture(
  status: OffsiteBackupStatus = disabledBackupStatus(),
): MockBackupGateway {
  return {
    backupNow: vi.fn(async () => backupOperation()),
    changeTarget: vi.fn(async () => backupOperation()),
    disable: vi.fn(async () => backupOperation()),
    listenToProgress: vi.fn(async () => () => undefined),
    loadStatus: vi.fn(async () => structuredClone(status)),
    removeCredentials: vi.fn(async () => backupOperation()),
    replaceCredentials: vi.fn(async () => backupOperation()),
    runRestoreDrill: vi.fn(async () => backupOperation()),
    takeOverRestoredBackup: vi.fn(async () => backupOperation()),
    testAndEnable: vi.fn(async () => backupOperation()),
  } as unknown as MockBackupGateway
}

function backupOperation(): OffsiteBackupOperation {
  return {
    operationId: '019f547b-6200-7000-8000-000000000099',
    operation: OffsiteBackupOperationKind.BackupNow,
    reused: false,
  }
}

function disabledBackupStatus(): OffsiteBackupStatus {
  return {
    configured: false,
    enabled: false,
    revision: null,
    target: null,
    credentials: CredentialAvailability.Missing,
    relational: {
      phase: RelationalBackupPhase.Off,
      latestLocalTxid: null,
      latestRemoteTxid: null,
      lastRemoteConfirmedAt: null,
      restartCount: 0,
      lastErrorCode: null,
    },
    media: {
      phase: MediaBackupPhase.Off,
      pendingCount: 0,
      pendingBytes: 0,
      retryWaitCount: 0,
      verifiedCount: 0,
      verifiedBytes: 0,
      blockedCount: 0,
      lastErrorCode: null,
    },
    checkpoint: {
      phase: CheckpointBackupPhase.Off,
      inProgressCheckpointId: null,
      lastCompleteCheckpointId: null,
      lastCompleteAt: null,
      lastErrorCode: null,
    },
    lastRestoreDrill: null,
    lastRestoreDrillAt: null,
    lastRestoreDrillError: null,
    takeoverAvailable: false,
    credentialCleanupPending: false,
    activeOperation: null,
  }
}

function enabledBackupStatus(): OffsiteBackupStatus {
  return {
    ...disabledBackupStatus(),
    configured: true,
    enabled: true,
    revision: 3,
    target: {
      accountId: '0123456789abcdef0123456789abcdef',
      jurisdiction: R2Jurisdiction.Default,
      bucket: 'dara-local',
      prefix: 'dara/primary',
    },
    credentials: CredentialAvailability.Present,
    relational: {
      phase: RelationalBackupPhase.Running,
      latestLocalTxid: '000000000000000a',
      latestRemoteTxid: '000000000000000a',
      lastRemoteConfirmedAt: 1_788_512_400_000,
      restartCount: 0,
      lastErrorCode: null,
    },
    media: {
      phase: MediaBackupPhase.Idle,
      pendingCount: 0,
      pendingBytes: 0,
      retryWaitCount: 0,
      verifiedCount: 12,
      verifiedBytes: 2_048,
      blockedCount: 0,
      lastErrorCode: null,
    },
    checkpoint: {
      phase: CheckpointBackupPhase.Idle,
      inProgressCheckpointId: null,
      lastCompleteCheckpointId: null,
      lastCompleteAt: null,
      lastErrorCode: null,
    },
  }
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
