import type { BackupErrorCode, R2Jurisdiction } from '../backup/index.ts'

export const ApplicationLaunchMode = {
  Normal: 'NORMAL',
  Recovery: 'RECOVERY',
} as const

export type ApplicationLaunchMode =
  (typeof ApplicationLaunchMode)[keyof typeof ApplicationLaunchMode]

export interface ApplicationLaunchContext {
  mode: ApplicationLaunchMode
}

export const RemoteCheckpointAvailability = {
  Restorable: 'RESTORABLE',
  ExactTxidUnavailable: 'EXACT_TXID_UNAVAILABLE',
} as const

export type RemoteCheckpointAvailability =
  (typeof RemoteCheckpointAvailability)[keyof typeof RemoteCheckpointAvailability]

export interface RemoteCheckpointSummary {
  checkpointId: string
  createdAt: string
  daraVersion: string
  txid: string
  mainMigrationHead: number
  mediaMigrationHead: number
  referencedMediaCount: number
  referencedMediaBytes: number
  availability: RemoteCheckpointAvailability
}

export interface RemoteCheckpointCatalog {
  checkpoints: RemoteCheckpointSummary[]
  malformedObjectsIgnored: number
  backupSetId: string
}

export interface DiscoverRemoteBackupsInput {
  accountId: string
  jurisdiction: R2Jurisdiction
  bucket: string
  credentials: {
    accessKeyId: string
    secretAccessKey: string
  }
}

export interface RestoreRemoteBackupInput {
  checkpointId: string
}

export const RecoveryCommandErrorCode = {
  InvalidInput: 'INVALID_INPUT',
  NotFreshInstall: 'NOT_FRESH_INSTALL',
  OperationInProgress: 'OPERATION_IN_PROGRESS',
  DiscoveryRequired: 'DISCOVERY_REQUIRED',
  BackupFailed: 'BACKUP_FAILED',
  Internal: 'INTERNAL',
} as const

export type RecoveryCommandErrorCode =
  (typeof RecoveryCommandErrorCode)[keyof typeof RecoveryCommandErrorCode]

export interface RecoveryCommandError {
  code: RecoveryCommandErrorCode
  backupErrorCode: BackupErrorCode | null
  message: string
}
