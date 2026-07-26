import type { MediaMaintenanceReport } from '../media/gateway.ts'
import type { SemanticSearchPhase } from '../review/index.ts'
import type {
  SchedulerAlgorithm,
  SchedulerLibrary,
} from '../scheduling/index.ts'

export interface DiagnosticsSnapshot {
  applicationVersion: string
  database: {
    migrationHeads: {
      main: number | null
      media: number | null
    }
    scheduler: {
      algorithm: SchedulerAlgorithm
      algorithmVersion: number
      schedulerLibrary: SchedulerLibrary
      libraryVersion: string
      desiredRetention: number
    }
    semanticIndex: {
      id: string
      active: boolean
      indexedDocuments: number
      totalDocuments: number
    }
  }
  semanticModel: {
    modelName: string
    phase: SemanticSearchPhase
    downloadedBytes: number
    expectedBytes: number
    lastError: string | null
  }
  storage: {
    relationalDatabaseBytes: number
    mediaDatabaseBytes: number
    modelBytes: number
    snapshotsBytes: number
    logsBytes: number
  }
  latestSnapshot: {
    createdAt: number
    applicationVersion: string
  } | null
  lastMediaMaintenance: MediaMaintenanceReport | null
}
