import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
import type {
  ApplicationLaunchContext,
  DiscoverRemoteBackupsInput,
  RemoteCheckpointCatalog,
  RestoreRemoteBackupInput,
} from './types.ts'

export interface FreshInstallRecoveryGateway {
  loadLaunchContext(): Promise<ApplicationLaunchContext>
  startFresh(): Promise<void>
  discover(input: DiscoverRemoteBackupsInput): Promise<RemoteCheckpointCatalog>
  restore(input: RestoreRemoteBackupInput): Promise<void>
}

export const tauriFreshInstallRecoveryGateway: FreshInstallRecoveryGateway = {
  loadLaunchContext: () =>
    invoke<ApplicationLaunchContext>(
      DaraIpcCommand.LoadApplicationLaunchContext,
    ),
  startFresh: () => invoke<void>(DaraIpcCommand.StartFreshInstall),
  discover: (input) =>
    invoke<RemoteCheckpointCatalog>(DaraIpcCommand.DiscoverRemoteBackups, {
      input,
    }),
  restore: (input) =>
    invoke<void>(DaraIpcCommand.RestoreRemoteBackup, { input }),
}
