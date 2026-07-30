import { afterEach, describe, expect, test } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { DaraIpcCommand } from '../../../src/lib/tauri-contracts.ts'
import { tauriDiagnosticsGateway } from '../../../src/diagnostics/index.ts'
import { native } from '../../../src/lib/native.ts'
import {
  ingestClipboardImage,
  ingestImageBytes,
  maintainMedia,
  renewMediaLease,
} from '../../../src/media/gateway.ts'
import {
  createCardContent,
  deleteCardContent,
  loadCardContent,
  loadHomeStats,
  maintainSearch,
  searchCardContent,
  searchStatus,
  setCardContentSuspended,
  tauriReviewGateway,
  updateCardContent,
} from '../../../src/review/gateway.ts'
import { SearchMaintenanceOperation } from '../../../src/review/contracts.ts'
import {
  SchedulerReplayInstallOperation,
  tauriSchedulerMaintenanceGateway,
} from '../../../src/scheduling/maintenance.ts'
import { Appearance, DaraCommand } from '../../../src/settings/types.ts'
import { tauriSettingsGateway } from '../../../src/settings/gateway.ts'
import {
  loadRestoredOffsiteBackupTakeoverRequired,
  R2Jurisdiction,
  tauriOffsiteBackupGateway,
} from '../../../src/backup/index.ts'

interface CapturedInvocation {
  command: string
  payload: unknown
}

const MEDIA_LEASE_ID = '01980c8e-6c00-7000-8000-000000000101'

afterEach(() => clearMocks())

describe('Tauri gateway contracts', () => {
  test.each([
    [DaraIpcCommand.SelectNextReviewCard, (input: never) => tauriReviewGateway.selectNextReviewCard(input)],
    [DaraIpcCommand.RecordGrade, (input: never) => tauriReviewGateway.recordGrade(input)],
    [DaraIpcCommand.UndoLastGrade, (input: never) => tauriReviewGateway.undoLastGrade(input)],
    [DaraIpcCommand.SearchCardContent, (input: never) => searchCardContent(input)],
    [DaraIpcCommand.SetCardContentSuspended, (input: never) => setCardContentSuspended(input)],
    [DaraIpcCommand.DeleteCardContent, (input: never) => deleteCardContent(input)],
    [DaraIpcCommand.LoadHomeStats, (input: never) => loadHomeStats(input)],
  ] as const)('%s sends the exact input envelope', async (command, invokeGateway) => {
    const input = { contractMarker: command } as never
    const invocation = await captureInvocation(() => invokeGateway(input))
    expect(invocation).toEqual({ command, payload: { input } })
  })

  test('create and update preserve camelCase media lease envelopes', async () => {
    const createInput = { contractMarker: 'create' } as never
    expect(
      await captureInvocation(() => createCardContent(createInput, MEDIA_LEASE_ID)),
    ).toEqual({
      command: DaraIpcCommand.CreateCardContent,
      payload: { input: createInput, mediaLeaseId: MEDIA_LEASE_ID },
    })

    const updateInput = { contractMarker: 'update' } as never
    expect(
      await captureInvocation(() => updateCardContent(updateInput, MEDIA_LEASE_ID)),
    ).toEqual({
      command: DaraIpcCommand.UpdateCardContent,
      payload: { input: updateInput, mediaLeaseId: MEDIA_LEASE_ID },
    })
  })

  test('load card content uses the addressable card ID envelope', async () => {
    const cardContentId = '01980c8e-6c00-7000-8000-000000000102'
    expect(
      await captureInvocation(() => loadCardContent(cardContentId)),
    ).toEqual({
      command: DaraIpcCommand.LoadCardContent,
      payload: { cardContentId },
    })
  })

  test('search status and maintenance use their command-specific envelopes', async () => {
    expect(await captureInvocation(searchStatus)).toEqual({
      command: DaraIpcCommand.SearchStatus,
      payload: {},
    })
    expect(
      await captureInvocation(() =>
        maintainSearch(SearchMaintenanceOperation.IntegrityCheck),
      ),
    ).toEqual({
      command: DaraIpcCommand.MaintainSearch,
      payload: { operation: SearchMaintenanceOperation.IntegrityCheck },
    })
  })

  test('settings commands preserve camelCase nested input envelopes', async () => {
    expect(await captureInvocation(tauriSettingsGateway.loadSettings)).toEqual({
      command: DaraIpcCommand.LoadSettings,
      payload: {},
    })
    expect(
      await captureInvocation(() =>
        tauriSettingsGateway.adoptLegacyZoom(7, 120),
      ),
    ).toEqual({
      command: DaraIpcCommand.AdoptLegacyZoom,
      payload: { input: { expectedRevision: 7, zoomPercent: 120 } },
    })
    expect(
      await captureInvocation(() =>
        tauriSettingsGateway.setAppearance(8, Appearance.Dark),
      ),
    ).toEqual({
      command: DaraIpcCommand.SetAppearance,
      payload: { input: { appearance: Appearance.Dark, expectedRevision: 8 } },
    })
    const keyboardBindings = [
      { accelerator: 'control+alt+super+KeyD', command: DaraCommand.QuickAdd },
    ]
    expect(
      await captureInvocation(() =>
        tauriSettingsGateway.setKeyboardBindings(9, keyboardBindings),
      ),
    ).toEqual({
      command: DaraIpcCommand.SetKeyboardBindings,
      payload: { input: { expectedRevision: 9, keyboardBindings } },
    })
    expect(
      await captureInvocation(() => tauriSettingsGateway.setLaunchAtLogin(true)),
    ).toEqual({
      command: DaraIpcCommand.SetLaunchAtLogin,
      payload: { input: { enabled: true } },
    })
    expect(
      await captureInvocation(() => tauriSettingsGateway.setZoomPercent(10, 130)),
    ).toEqual({
      command: DaraIpcCommand.SetZoomPercent,
      payload: { input: { expectedRevision: 10, zoomPercent: 130 } },
    })
  })

  test('diagnostics uses its dedicated read-only command', async () => {
    expect(
      await captureInvocation(tauriDiagnosticsGateway.loadDiagnostics),
    ).toEqual({
      command: DaraIpcCommand.LoadDiagnostics,
      payload: {},
    })
  })

  test('off-site backup commands preserve secret-bearing input envelopes', async () => {
    const target = {
      accountId: '0123456789abcdef0123456789abcdef',
      jurisdiction: R2Jurisdiction.Default,
      bucket: 'dara-local',
    }
    const credentials = {
      accessKeyId: '11111111111111111111111111111111',
      secretAccessKey:
        '2222222222222222222222222222222222222222222222222222222222222222',
    }
    expect(
      await captureInvocation(() =>
        tauriOffsiteBackupGateway.testAndEnable({ credentials, target }),
      ),
    ).toEqual({
      command: DaraIpcCommand.TestAndEnableOffsiteBackup,
      payload: { input: { credentials, target } },
    })
    expect(
      await captureInvocation(() =>
        tauriOffsiteBackupGateway.replaceCredentials({ credentials }),
      ),
    ).toEqual({
      command: DaraIpcCommand.ReplaceOffsiteBackupCredentials,
      payload: { input: { credentials } },
    })
    expect(
      await captureInvocation(() =>
        tauriOffsiteBackupGateway.changeTarget({ credentials, target }),
      ),
    ).toEqual({
      command: DaraIpcCommand.ChangeOffsiteBackupTarget,
      payload: { input: { credentials, target } },
    })
  })

  test('off-site backup actions use dedicated commands without secret output', async () => {
    const cases = [
      [
        DaraIpcCommand.LoadOffsiteBackupStatus,
        () => tauriOffsiteBackupGateway.loadStatus(),
        {},
      ],
      [
        DaraIpcCommand.LoadRestoredOffsiteBackupTakeoverRequired,
        () => loadRestoredOffsiteBackupTakeoverRequired(),
        {},
      ],
      [
        DaraIpcCommand.CreateOffsiteBackupNow,
        () => tauriOffsiteBackupGateway.backupNow(),
        {},
      ],
      [
        DaraIpcCommand.RunOffsiteRestoreDrill,
        () => tauriOffsiteBackupGateway.runRestoreDrill(),
        {},
      ],
      [
        DaraIpcCommand.DisableOffsiteBackup,
        () => tauriOffsiteBackupGateway.disable(),
        {},
      ],
      [
        DaraIpcCommand.RemoveOffsiteBackupCredentials,
        () => tauriOffsiteBackupGateway.removeCredentials(),
        {},
      ],
      [
        DaraIpcCommand.TakeOverRestoredOffsiteBackup,
        () => tauriOffsiteBackupGateway.takeOverRestoredBackup(),
        { input: { confirmed: true } },
      ],
    ] as const
    for (const [command, run, payload] of cases) {
      expect(await captureInvocation(run)).toEqual({ command, payload })
    }
  })

  test('scheduler maintenance commands preserve their envelopes', async () => {
    expect(
      await captureInvocation(
        tauriSchedulerMaintenanceGateway.loadSchedulerReplaySnapshot,
      ),
    ).toEqual({
      command: DaraIpcCommand.LoadSchedulerReplaySnapshot,
      payload: {},
    })
    expect(
      await captureInvocation(() =>
        tauriSchedulerMaintenanceGateway.prepareDesiredRetentionReplay(0.91),
      ),
    ).toEqual({
      command: DaraIpcCommand.PrepareDesiredRetentionReplay,
      payload: { input: { desiredRetention: 0.91 } },
    })
    const input = {
      operation: SchedulerReplayInstallOperation.Repair,
      contractMarker: 'install',
    } as never
    expect(
      await captureInvocation(() =>
        tauriSchedulerMaintenanceGateway.installSchedulerReplay(input),
      ),
    ).toEqual({
      command: DaraIpcCommand.InstallSchedulerReplay,
      payload: { input },
    })
  })

  test('native commands use exact command names and top-level envelopes', async () => {
    const cases = [
      [DaraIpcCommand.DismissQuickAdd, () => native.dismissQuickAdd(), {}],
      [DaraIpcCommand.OpenExternalUrl, () => native.openExternalUrl('https://example.test'), { url: 'https://example.test' }],
      [DaraIpcCommand.SetQuickAddFileDialogOpen, () => native.setQuickAddFileDialogOpen(true), { open: true }],
      [DaraIpcCommand.ShowMain, () => native.showMain(), {}],
      [DaraIpcCommand.ShowQuickAdd, () => native.showQuickAdd(), {}],
    ] as const
    for (const [command, run, payload] of cases) {
      expect(await captureInvocation(run)).toEqual({ command, payload })
    }
  })

  test('media commands preserve JSON envelopes and binary framing', async () => {
    expect(
      await captureInvocation(() => ingestClipboardImage(MEDIA_LEASE_ID)),
    ).toEqual({
      command: DaraIpcCommand.IngestClipboardImage,
      payload: { leaseId: MEDIA_LEASE_ID },
    })
    expect(
      await captureInvocation(() => renewMediaLease(MEDIA_LEASE_ID)),
    ).toEqual({
      command: DaraIpcCommand.RenewMediaLease,
      payload: { leaseId: MEDIA_LEASE_ID },
    })
    expect(await captureInvocation(maintainMedia)).toEqual({
      command: DaraIpcCommand.MaintainMedia,
      payload: {},
    })

    const invocation = await captureInvocation(() =>
      ingestImageBytes(new Uint8Array([11, 22, 33]), MEDIA_LEASE_ID),
    )
    expect(invocation.command).toBe(DaraIpcCommand.IngestImageBytes)
    expect(invocation.payload).toBeInstanceOf(Uint8Array)
    expect(Array.from(invocation.payload as Uint8Array)).toEqual([
      ...new TextEncoder().encode(MEDIA_LEASE_ID),
      11,
      22,
      33,
    ])
  })
})

async function captureInvocation(run: () => Promise<unknown>): Promise<CapturedInvocation> {
  let invocation: CapturedInvocation | undefined
  mockIPC((command, payload) => {
    invocation = { command, payload }
    return null
  })
  await run()
  if (!invocation) {
    throw new Error('Expected one Tauri invocation.')
  }
  return invocation
}
