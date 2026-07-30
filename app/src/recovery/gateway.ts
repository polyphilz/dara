import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
import type {
  ApplicationLaunchContext,
  DiscoverRemoteBackupsInput,
  RemoteCheckpointCatalog,
} from './types.ts'

export interface FreshInstallRecoveryGateway {
  loadLaunchContext(): Promise<ApplicationLaunchContext>
  startFresh(): Promise<void>
  discover(input: DiscoverRemoteBackupsInput): Promise<RemoteCheckpointCatalog>
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
}
