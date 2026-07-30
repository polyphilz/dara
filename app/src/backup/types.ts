export const R2Jurisdiction = {
  Default: 'DEFAULT',
  Eu: 'EU',
  Fedramp: 'FEDRAMP',
} as const

export type R2Jurisdiction =
  (typeof R2Jurisdiction)[keyof typeof R2Jurisdiction]

export const CredentialAvailability = {
  Present: 'PRESENT',
  Missing: 'MISSING',
  Unavailable: 'UNAVAILABLE',
} as const

export type CredentialAvailability =
  (typeof CredentialAvailability)[keyof typeof CredentialAvailability]

export const RelationalBackupPhase = {
  Off: 'OFF',
  WaitingForCredentials: 'WAITING_FOR_CREDENTIALS',
  Starting: 'STARTING',
  Running: 'RUNNING',
  Degraded: 'DEGRADED',
  Blocked: 'BLOCKED',
  Unavailable: 'UNAVAILABLE',
} as const

export type RelationalBackupPhase =
  (typeof RelationalBackupPhase)[keyof typeof RelationalBackupPhase]

export const MediaBackupPhase = {
  Off: 'OFF',
  WaitingForCredentials: 'WAITING_FOR_CREDENTIALS',
  Idle: 'IDLE',
  Uploading: 'UPLOADING',
  RetryWait: 'RETRY_WAIT',
  Blocked: 'BLOCKED',
  Unavailable: 'UNAVAILABLE',
} as const

export type MediaBackupPhase =
  (typeof MediaBackupPhase)[keyof typeof MediaBackupPhase]

export const CheckpointBackupPhase = {
  Off: 'OFF',
  WaitingForMedia: 'WAITING_FOR_MEDIA',
  Fencing: 'FENCING',
  WaitingForReplica: 'WAITING_FOR_REPLICA',
  Validating: 'VALIDATING',
  Publishing: 'PUBLISHING',
  Idle: 'IDLE',
  Degraded: 'DEGRADED',
  Blocked: 'BLOCKED',
  Unavailable: 'UNAVAILABLE',
} as const

export type CheckpointBackupPhase =
  (typeof CheckpointBackupPhase)[keyof typeof CheckpointBackupPhase]

export const BackupErrorCode = {
  NetworkOffline: 'NETWORK_OFFLINE',
  NetworkTimeout: 'NETWORK_TIMEOUT',
  RateLimited: 'RATE_LIMITED',
  ServiceUnavailable: 'SERVICE_UNAVAILABLE',
  KeychainCredentialMissing: 'KEYCHAIN_CREDENTIAL_MISSING',
  KeychainUnavailable: 'KEYCHAIN_UNAVAILABLE',
  InvalidTarget: 'INVALID_TARGET',
  AuthenticationRejected: 'AUTHENTICATION_REJECTED',
  AuthorizationRejected: 'AUTHORIZATION_REJECTED',
  PrefixIdentityMismatch: 'PREFIX_IDENTITY_MISMATCH',
  OwnerMismatch: 'OWNER_MISMATCH',
  ImmutableObjectConflict: 'IMMUTABLE_OBJECT_CONFLICT',
  LocalMediaMissing: 'LOCAL_MEDIA_MISSING',
  LocalMediaTooLarge: 'LOCAL_MEDIA_TOO_LARGE',
  LocalMediaHashMismatch: 'LOCAL_MEDIA_HASH_MISMATCH',
  WorkerUnavailable: 'WORKER_UNAVAILABLE',
  LitestreamUnavailable: 'LITESTREAM_UNAVAILABLE',
  LitestreamFailed: 'LITESTREAM_FAILED',
  FenceTimeout: 'FENCE_TIMEOUT',
  ReplicaBehind: 'REPLICA_BEHIND',
  CheckpointNotFound: 'CHECKPOINT_NOT_FOUND',
  ExactTxidUnavailable: 'EXACT_TXID_UNAVAILABLE',
  MalformedManifest: 'MALFORMED_MANIFEST',
  RemoteMediaMissing: 'REMOTE_MEDIA_MISSING',
  RemoteMediaCorrupt: 'REMOTE_MEDIA_CORRUPT',
  RestoreValidationFailed: 'RESTORE_VALIDATION_FAILED',
} as const

export type BackupErrorCode =
  (typeof BackupErrorCode)[keyof typeof BackupErrorCode]

export const OffsiteBackupOperationKind = {
  TestAndEnable: 'TEST_AND_ENABLE',
  ReplaceCredentials: 'REPLACE_CREDENTIALS',
  ChangeTarget: 'CHANGE_TARGET',
  TakeOver: 'TAKE_OVER',
  Disable: 'DISABLE',
  RemoveCredentials: 'REMOVE_CREDENTIALS',
  BackupNow: 'BACKUP_NOW',
  RestoreDrill: 'RESTORE_DRILL',
} as const

export type OffsiteBackupOperationKind =
  (typeof OffsiteBackupOperationKind)[keyof typeof OffsiteBackupOperationKind]

export const OffsiteBackupProgressPhase = {
  ValidatingConfig: 'VALIDATING_CONFIG',
  TestingObjectStore: 'TESTING_OBJECT_STORE',
  TestingLitestream: 'TESTING_LITESTREAM',
  SavingConfiguration: 'SAVING_CONFIGURATION',
  ReconcilingMedia: 'RECONCILING_MEDIA',
  FencingDatabase: 'FENCING_DATABASE',
  WaitingForReplication: 'WAITING_FOR_REPLICATION',
  PublishingCheckpoint: 'PUBLISHING_CHECKPOINT',
  RestoringRelational: 'RESTORING_RELATIONAL',
  RestoringMedia: 'RESTORING_MEDIA',
  ValidatingPair: 'VALIDATING_PAIR',
  Complete: 'COMPLETE',
  Failed: 'FAILED',
} as const

export type OffsiteBackupProgressPhase =
  (typeof OffsiteBackupProgressPhase)[keyof typeof OffsiteBackupProgressPhase]

export const RestoreDrillOutcome = {
  Success: 'SUCCESS',
  Failed: 'FAILED',
} as const

export type RestoreDrillOutcome =
  (typeof RestoreDrillOutcome)[keyof typeof RestoreDrillOutcome]

export const RestoreValidationStage = {
  CheckpointDiscovered: 'CHECKPOINT_DISCOVERED',
  ExactTxidRestored: 'EXACT_TXID_RESTORED',
  RelationalValidated: 'RELATIONAL_VALIDATED',
  MediaReconstructed: 'MEDIA_RECONSTRUCTED',
  PairValidated: 'PAIR_VALIDATED',
} as const

export type RestoreValidationStage =
  (typeof RestoreValidationStage)[keyof typeof RestoreValidationStage]

export interface OffsiteBackupTarget {
  accountId: string
  jurisdiction: R2Jurisdiction
  bucket: string
}

export interface OffsiteBackupCredentials {
  accessKeyId: string
  secretAccessKey: string
}

export interface RelationalBackupStatus {
  phase: RelationalBackupPhase
  latestLocalTxid: string | null
  latestRemoteTxid: string | null
  lastRemoteConfirmedAt: number | null
  restartCount: number
  lastErrorCode: BackupErrorCode | null
}

export interface MediaBackupStatus {
  phase: MediaBackupPhase
  pendingCount: number
  pendingBytes: number
  retryWaitCount: number
  verifiedCount: number
  verifiedBytes: number
  blockedCount: number
  lastErrorCode: BackupErrorCode | null
}

export interface CheckpointBackupStatus {
  phase: CheckpointBackupPhase
  inProgressCheckpointId: string | null
  lastCompleteCheckpointId: string | null
  lastCompleteAt: number | null
  lastErrorCode: BackupErrorCode | null
}

export interface RestoreDrillReport {
  formatVersion: number
  backupSetId: string | null
  replicaEpochId: string | null
  outcome: RestoreDrillOutcome
  checkpointId: string | null
  checkpointCreatedAt: string | null
  restoredTxid: string | null
  mainMigrationHead: number | null
  mediaMigrationHead: number | null
  referencedMediaCount: number | null
  referencedMediaBytes: number | null
  validationStages: RestoreValidationStage[]
  durationMs: number
  daraVersion: string
  errorCode: BackupErrorCode | null
}

export interface OffsiteBackupOperation {
  operationId: string
  operation: OffsiteBackupOperationKind
  reused: boolean
}

export interface OffsiteBackupProgress {
  operationId: string
  operation: OffsiteBackupOperationKind
  phase: OffsiteBackupProgressPhase
  errorCode: BackupErrorCode | null
}

export interface OffsiteBackupStatus {
  configured: boolean
  enabled: boolean
  revision: number | null
  target: OffsiteBackupTarget | null
  credentials: CredentialAvailability
  relational: RelationalBackupStatus
  media: MediaBackupStatus
  checkpoint: CheckpointBackupStatus
  lastRestoreDrill: RestoreDrillReport | null
  lastRestoreDrillAt: number | null
  lastRestoreDrillError: BackupErrorCode | null
  takeoverAvailable: boolean
  credentialCleanupPending: boolean
  activeOperation: OffsiteBackupOperation | null
}

export interface TestAndEnableOffsiteBackupInput {
  target: OffsiteBackupTarget
  credentials: OffsiteBackupCredentials
}

export interface ReplaceOffsiteBackupCredentialsInput {
  credentials: OffsiteBackupCredentials
}

export interface ChangeOffsiteBackupTargetInput {
  target: OffsiteBackupTarget
  credentials: OffsiteBackupCredentials
}
