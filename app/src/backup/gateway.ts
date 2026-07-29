import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { DaraEvent, DaraIpcCommand } from '../lib/tauri-contracts.ts'
import type {
  ChangeOffsiteBackupTargetInput,
  OffsiteBackupOperation,
  OffsiteBackupProgress,
  OffsiteBackupStatus,
  ReplaceOffsiteBackupCredentialsInput,
  TestAndEnableOffsiteBackupInput,
} from './types.ts'

export interface OffsiteBackupGateway {
  backupNow(): Promise<OffsiteBackupOperation>
  changeTarget(
    input: ChangeOffsiteBackupTargetInput,
  ): Promise<OffsiteBackupOperation>
  disable(): Promise<OffsiteBackupOperation>
  listenToProgress(
    listener: (progress: OffsiteBackupProgress) => void,
  ): Promise<() => void>
  loadStatus(): Promise<OffsiteBackupStatus>
  removeCredentials(): Promise<OffsiteBackupOperation>
  replaceCredentials(
    input: ReplaceOffsiteBackupCredentialsInput,
  ): Promise<OffsiteBackupOperation>
  runRestoreDrill(): Promise<OffsiteBackupOperation>
  takeOverRestoredBackup(): Promise<OffsiteBackupOperation>
  testAndEnable(
    input: TestAndEnableOffsiteBackupInput,
  ): Promise<OffsiteBackupOperation>
}

export const loadRestoredOffsiteBackupTakeoverRequired = () =>
  invoke<boolean>(DaraIpcCommand.LoadRestoredOffsiteBackupTakeoverRequired)

export const tauriOffsiteBackupGateway: OffsiteBackupGateway = {
  backupNow: () =>
    invoke<OffsiteBackupOperation>(DaraIpcCommand.CreateOffsiteBackupNow),
  changeTarget: (input) =>
    invoke<OffsiteBackupOperation>(DaraIpcCommand.ChangeOffsiteBackupTarget, {
      input,
    }),
  disable: () =>
    invoke<OffsiteBackupOperation>(DaraIpcCommand.DisableOffsiteBackup),
  listenToProgress: (listener) =>
    listen<OffsiteBackupProgress>(
      DaraEvent.OffsiteBackupProgress,
      (event) => listener(event.payload),
    ),
  loadStatus: () =>
    invoke<OffsiteBackupStatus>(DaraIpcCommand.LoadOffsiteBackupStatus),
  removeCredentials: () =>
    invoke<OffsiteBackupOperation>(
      DaraIpcCommand.RemoveOffsiteBackupCredentials,
    ),
  replaceCredentials: (input) =>
    invoke<OffsiteBackupOperation>(
      DaraIpcCommand.ReplaceOffsiteBackupCredentials,
      { input },
    ),
  runRestoreDrill: () =>
    invoke<OffsiteBackupOperation>(DaraIpcCommand.RunOffsiteRestoreDrill),
  takeOverRestoredBackup: () =>
    invoke<OffsiteBackupOperation>(
      DaraIpcCommand.TakeOverRestoredOffsiteBackup,
      { input: { confirmed: true } },
    ),
  testAndEnable: (input) =>
    invoke<OffsiteBackupOperation>(
      DaraIpcCommand.TestAndEnableOffsiteBackup,
      { input },
    ),
}
