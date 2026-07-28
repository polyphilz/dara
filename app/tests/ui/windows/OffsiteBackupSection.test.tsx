import {
  act,
  fireEvent,
  render,
  waitFor,
  within,
} from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import {
  BackupErrorCode,
  CheckpointBackupPhase,
  CredentialAvailability,
  MediaBackupPhase,
  OffsiteBackupOperationKind,
  OffsiteBackupProgressPhase,
  R2Jurisdiction,
  RelationalBackupPhase,
  RestoreDrillOutcome,
  type OffsiteBackupGateway,
  type OffsiteBackupProgress,
  type OffsiteBackupStatus,
} from '../../../src/backup/index.ts'
import { OffsiteBackupSection } from '../../../src/windows/main/OffsiteBackupSection.tsx'

const ACCESS_KEY_ID = '11111111111111111111111111111111'
const SECRET_ACCESS_KEY =
  '2222222222222222222222222222222222222222222222222222222222222222'

beforeEach(() => {
  vi.clearAllMocks()
})

test('starts off, explains local independence, and never implies client-side encryption', async () => {
  const fixture = backupFixture(disabledStatus())
  const { findByRole, findByText, getByLabelText } = renderSection(fixture.gateway)

  expect(await findByRole('button', { name: 'Test and enable backup' })).toBeTruthy()
  expect(
    await findByText(/network or backup problem will not block normal use/i),
  ).toBeTruthy()
  expect(await findByText(/not encrypted by Dara before upload/i)).toBeTruthy()
  expect((getByLabelText('Prefix') as HTMLInputElement).value).toBe(
    'dara/primary',
  )
  expect(fixture.gateway.backupNow).not.toHaveBeenCalled()
})

test('validates locally, sends credentials once, and clears both credential fields after failure', async () => {
  const fixture = backupFixture(disabledStatus())
  fixture.gateway.testAndEnable.mockRejectedValueOnce({
    code: BackupErrorCode.AuthenticationRejected,
    message: 'Cloudflare rejected the R2 credentials.',
  })
  const { findByRole, findByText, getByLabelText } = renderSection(fixture.gateway)

  const submit = await findByRole('button', { name: 'Test and enable backup' })
  fireEvent.click(submit)
  expect(
    await findByText('Enter the 32-character lowercase R2 account ID.'),
  ).toBeTruthy()
  expect(fixture.gateway.testAndEnable).not.toHaveBeenCalled()

  fireEvent.change(getByLabelText('Account ID'), {
    target: { value: '0123456789abcdef0123456789abcdef' },
  })
  fireEvent.change(getByLabelText('Bucket'), {
    target: { value: 'dara-local' },
  })
  fireEvent.change(getByLabelText('Access Key ID'), {
    target: { value: ACCESS_KEY_ID },
  })
  fireEvent.change(getByLabelText('Secret Access Key'), {
    target: { value: SECRET_ACCESS_KEY },
  })
  fireEvent.click(submit)

  expect(
    await findByText('Cloudflare rejected the R2 credentials.'),
  ).toBeTruthy()
  expect(fixture.gateway.testAndEnable).toHaveBeenCalledWith({
    credentials: {
      accessKeyId: ACCESS_KEY_ID,
      secretAccessKey: SECRET_ACCESS_KEY,
    },
    target: {
      accountId: '0123456789abcdef0123456789abcdef',
      jurisdiction: R2Jurisdiction.Default,
      bucket: 'dara-local',
      prefix: 'dara/primary',
    },
  })
  expect((getByLabelText('Access Key ID') as HTMLInputElement).value).toBe('')
  expect((getByLabelText('Secret Access Key') as HTMLInputElement).value).toBe(
    '',
  )
  expect(fixture.gateway.loadStatus).toHaveBeenCalledTimes(2)
})

test('keeps component freshness separate from a complete recoverable checkpoint', async () => {
  const fixture = backupFixture(enabledStatus({ complete: false }))
  const { findByRole, findByText } = renderSection(fixture.gateway)

  expect(await findByText('Running')).toBeTruthy()
  expect(await findByText('Up to date')).toBeTruthy()
  expect(await findByText('Not ready')).toBeTruthy()
  expect(
    await findByText(/no complete recoverable checkpoint yet/i),
  ).toBeTruthy()
  expect(
    (await findByRole('button', {
      name: 'Run restore drill',
    })) as HTMLButtonElement,
  ).toHaveProperty('disabled', true)
})

test('shows the last complete checkpoint and durable restore-drill result', async () => {
  const fixture = backupFixture(enabledStatus({ complete: true }))
  const { findByRole, findByText } = renderSection(fixture.gateway)

  expect(await findByText(/checkpoint 019f547b/)).toBeTruthy()
  expect(await findByText('Passed')).toBeTruthy()
  expect(
    (await findByRole('button', {
      name: 'Run restore drill',
    })) as HTMLButtonElement,
  ).toHaveProperty('disabled', false)
})

test('announces typed progress and reloads after completion', async () => {
  const fixture = backupFixture(enabledStatus({ complete: true }))
  const { findByRole, findByText } = renderSection(fixture.gateway)
  const backupButton = await findByRole('button', { name: 'Back up now' })

  let resolveBackup: (() => void) | undefined
  fixture.gateway.backupNow.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        resolveBackup = () =>
          resolve({
            operationId: '019f547b-6200-7000-8000-000000000099',
            operation: OffsiteBackupOperationKind.BackupNow,
            reused: false,
          })
      }),
  )
  fireEvent.click(backupButton)
  await act(async () => {
    fixture.progress?.({
      errorCode: null,
      operation: OffsiteBackupOperationKind.BackupNow,
      operationId: '019f547b-6200-7000-8000-000000000099',
      phase: OffsiteBackupProgressPhase.WaitingForReplication,
    })
  })
  expect(await findByText('Waiting for R2 to catch up…')).toBeTruthy()

  await act(async () => {
    resolveBackup?.()
  })
  expect(
    await findByText('A complete recoverable backup was created.'),
  ).toBeTruthy()
  expect(fixture.gateway.loadStatus).toHaveBeenCalledTimes(2)
})

test('confirms credential removal, keeps remote data, and restores focus', async () => {
  const fixture = backupFixture(enabledStatus({ complete: true }))
  const { findByRole, findByText, getByRole, queryByRole } = renderSection(
    fixture.gateway,
  )

  const remove = await findByRole('button', {
    name: 'Remove saved credentials',
  })
  fireEvent.click(remove)
  expect(
    getByRole('alertdialog', { name: 'Remove saved R2 credentials?' }),
  ).toBeTruthy()
  expect(await findByText(/will not delete anything from R2/i)).toBeTruthy()
  fireEvent.click(getByRole('button', { name: 'Cancel' }))

  expect(queryByRole('alertdialog')).toBeNull()
  await waitFor(() => expect(document.activeElement).toBe(remove))
  expect(fixture.gateway.removeCredentials).not.toHaveBeenCalled()
})

test('keeps credential removal available after backup is disabled', async () => {
  const fixture = backupFixture(configuredDisabledStatus())
  const { findByRole, getByRole } = renderSection(fixture.gateway)

  expect(
    await findByRole('button', { name: 'Test and re-enable backup' }),
  ).toBeTruthy()
  fireEvent.click(
    await findByRole('button', { name: 'Remove saved credentials' }),
  )
  expect(
    getByRole('alertdialog', { name: 'Remove saved R2 credentials?' }),
  ).toBeTruthy()
})

test('offers explicit takeover after a disabled restored backup detects another owner', async () => {
  const disabled = configuredDisabledStatus()
  const fixture = backupFixture(disabled)
  fixture.gateway.loadStatus
    .mockResolvedValueOnce(structuredClone(disabled))
    .mockResolvedValue({
      ...structuredClone(disabled),
      takeoverAvailable: true,
    })
  fixture.gateway.testAndEnable.mockRejectedValueOnce({
    code: BackupErrorCode.OwnerMismatch,
    message: 'Another Dara installation currently owns this backup.',
  })
  const { findByLabelText, findByRole, findByText, getByRole } = renderSection(
    fixture.gateway,
  )

  fireEvent.change(await findByLabelText('Access Key ID'), {
    target: { value: ACCESS_KEY_ID },
  })
  fireEvent.change(await findByLabelText('Secret Access Key'), {
    target: { value: SECRET_ACCESS_KEY },
  })
  fireEvent.click(
    await findByRole('button', { name: 'Test and re-enable backup' }),
  )
  expect(
    await findByText('Another Dara installation currently owns this backup.'),
  ).toBeTruthy()

  fireEvent.click(
    await findByRole('button', { name: 'Take over restored backup' }),
  )
  expect(
    getByRole('alertdialog', { name: 'Take over this restored backup?' }),
  ).toBeTruthy()
})

test('confirms a new target before invoking the backend and returns focus on cancel', async () => {
  const fixture = backupFixture(enabledStatus({ complete: true }))
  const {
    findByLabelText,
    findByRole,
    getByRole,
    queryByRole,
  } = renderSection(fixture.gateway)

  fireEvent.click(await findByRole('button', { name: 'Change target' }))
  fireEvent.change(await findByLabelText('Bucket'), {
    target: { value: 'dara-new-target' },
  })
  fireEvent.change(await findByLabelText('Access Key ID'), {
    target: { value: ACCESS_KEY_ID },
  })
  fireEvent.change(await findByLabelText('Secret Access Key'), {
    target: { value: SECRET_ACCESS_KEY },
  })
  const submit = await findByRole('button', {
    name: 'Test and change target',
  })
  fireEvent.click(submit)

  const dialog = getByRole('alertdialog', {
    name: 'Change off-site backup target?',
  })
  expect(dialog).toBeTruthy()
  expect(fixture.gateway.changeTarget).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))

  expect(queryByRole('alertdialog')).toBeNull()
  await waitFor(() => expect(document.activeElement).toBe(submit))
})

test('preserves target edits when status refreshes', async () => {
  const fixture = backupFixture(enabledStatus({ complete: true }))
  const { findByLabelText, findByRole } = renderSection(fixture.gateway)

  fireEvent.click(await findByRole('button', { name: 'Change target' }))
  const bucket = await findByLabelText('Bucket')
  fireEvent.change(bucket, {
    target: { value: 'dara-edited-target' },
  })
  await waitFor(() => expect(fixture.progress).toBeTypeOf('function'))
  await act(async () => {
    fixture.progress?.({
      errorCode: null,
      operation: OffsiteBackupOperationKind.ChangeTarget,
      operationId: '019f547b-6200-7000-8000-000000000099',
      phase: OffsiteBackupProgressPhase.Complete,
    })
  })
  await waitFor(() => expect(fixture.gateway.loadStatus).toHaveBeenCalledTimes(2))

  expect((bucket as HTMLInputElement).value).toBe('dara-edited-target')
})

type MockBackupGateway = OffsiteBackupGateway & {
  backupNow: ReturnType<typeof vi.fn>
  changeTarget: ReturnType<typeof vi.fn>
  disable: ReturnType<typeof vi.fn>
  listenToProgress: ReturnType<typeof vi.fn>
  loadStatus: ReturnType<typeof vi.fn>
  removeCredentials: ReturnType<typeof vi.fn>
  replaceCredentials: ReturnType<typeof vi.fn>
  runRestoreDrill: ReturnType<typeof vi.fn>
  takeOverRestoredBackup: ReturnType<typeof vi.fn>
  testAndEnable: ReturnType<typeof vi.fn>
}

function backupFixture(status: OffsiteBackupStatus): {
  gateway: MockBackupGateway
  progress?: (progress: OffsiteBackupProgress) => void
} {
  const fixture: {
    gateway: MockBackupGateway
    progress?: (progress: OffsiteBackupProgress) => void
  } = {
    gateway: {
      backupNow: vi.fn().mockResolvedValue(operation()),
      changeTarget: vi.fn().mockResolvedValue(operation()),
      disable: vi.fn().mockResolvedValue(operation()),
      listenToProgress: vi.fn(
        async (listener: (progress: OffsiteBackupProgress) => void) => {
          fixture.progress = listener
          return () => undefined
        },
      ),
      loadStatus: vi.fn(async () => structuredClone(status)),
      removeCredentials: vi.fn().mockResolvedValue(operation()),
      replaceCredentials: vi.fn().mockResolvedValue(operation()),
      runRestoreDrill: vi.fn().mockResolvedValue(operation()),
      takeOverRestoredBackup: vi.fn().mockResolvedValue(operation()),
      testAndEnable: vi.fn().mockResolvedValue(operation()),
    } as unknown as MockBackupGateway,
  }
  return fixture
}

function renderSection(gateway: OffsiteBackupGateway) {
  return render(
    <OffsiteBackupSection gateway={gateway} onBusyChange={vi.fn()} />,
  )
}

function operation() {
  return {
    operationId: '019f547b-6200-7000-8000-000000000099',
    operation: OffsiteBackupOperationKind.BackupNow,
    reused: false,
  }
}

function disabledStatus(): OffsiteBackupStatus {
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
    activeOperation: null,
  }
}

function enabledStatus({ complete }: { complete: boolean }): OffsiteBackupStatus {
  const status = disabledStatus()
  return {
    ...status,
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
      verifiedBytes: 2048,
      blockedCount: 0,
      lastErrorCode: null,
    },
    checkpoint: {
      phase: CheckpointBackupPhase.Idle,
      inProgressCheckpointId: null,
      lastCompleteCheckpointId: complete
        ? '019f547b-6200-7000-8000-000000000001'
        : null,
      lastCompleteAt: complete ? 1_788_512_400_000 : null,
      lastErrorCode: null,
    },
    lastRestoreDrill: complete
      ? {
          formatVersion: 2,
          backupSetId: '019f547b-6200-7000-8000-000000000010',
          replicaEpochId: '019f547b-6200-7000-8000-000000000011',
          outcome: RestoreDrillOutcome.Success,
          checkpointId: '019f547b-6200-7000-8000-000000000001',
          checkpointCreatedAt: '2026-09-03T12:00:00Z',
          restoredTxid: '000000000000000a',
          mainMigrationHead: 9,
          mediaMigrationHead: 4,
          referencedMediaCount: 12,
          referencedMediaBytes: 2048,
          validationStages: [],
          durationMs: 4321,
          daraVersion: '0.1.0',
          errorCode: null,
        }
      : null,
    lastRestoreDrillAt: complete ? 1_788_512_500_000 : null,
  }
}

function configuredDisabledStatus(): OffsiteBackupStatus {
  const status = enabledStatus({ complete: true })
  return {
    ...status,
    enabled: false,
    relational: {
      ...status.relational,
      phase: RelationalBackupPhase.Off,
    },
    media: {
      ...status.media,
      phase: MediaBackupPhase.Off,
    },
    checkpoint: {
      ...status.checkpoint,
      phase: CheckpointBackupPhase.Off,
    },
  }
}
