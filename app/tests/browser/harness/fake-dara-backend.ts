import {
  DaraIpcCommand,
  type DaraIpcCommand as DaraIpcCommandType,
} from '../../../src/lib/tauri-contracts.ts'
import {
  CardContentType,
  CardContentReviewStatus,
  MutationDisposition,
  ReviewCardStatus,
  ReviewQueueLane,
  ReviewQueueSelectionKind,
  SearchExecutionMode,
  SemanticSearchPhase,
  type CardContentDraft,
  type BasicCardContentDraft,
  type CardContentListItem,
  type RecordGradeInput,
  type ReviewContext,
} from '../../../src/review/contracts.ts'
import {
  DEFAULT_SCHEDULER_CONFIG,
  ReviewCardState,
  createNewReviewCardCache,
} from '../../../src/scheduling/index.ts'
import {
  Appearance,
  DEFAULT_KEYBOARD_BINDINGS,
} from '../../../src/settings/types.ts'
import {
  CheckpointBackupPhase,
  CredentialAvailability,
  MediaBackupPhase,
  OffsiteBackupOperationKind,
  R2Jurisdiction,
  RelationalBackupPhase,
  RestoreDrillOutcome,
  type OffsiteBackupStatus,
} from '../../../src/backup/index.ts'
import { ApplicationLaunchMode } from '../../../src/recovery/index.ts'
import type { BrowserScenario } from './scenarios.ts'
import { BrowserScenarioId } from './scenarios.ts'

export interface FakeCardRecord {
  content: CardContentDraft
  mediaLeaseId: string
}

export interface FakeCommandRecord {
  command: DaraIpcCommandType
  payload: unknown
}

export interface FakeBackendSnapshot {
  cards: FakeCardRecord[]
  commands: FakeCommandRecord[]
  dismissedQuickAdd: number
  recordedGrades: RecordGradeInput[]
}

const ipcCommands = new Set<DaraIpcCommandType>(Object.values(DaraIpcCommand))

export class FakeDaraBackend {
  readonly scenario: BrowserScenario
  readonly #cards: FakeCardRecord[] = []
  readonly #items: CardContentListItem[] = []
  readonly #commands: FakeCommandRecord[] = []
  readonly #recordedGrades: RecordGradeInput[] = []
  readonly #reviewContext: ReviewContext
  #offsiteBackupStatus = disabledBackupStatus()
  #restoredBackupTakeoverRequired = false
  #dismissedQuickAdd = 0
  #remainingCreateFailures = 0

  constructor(scenario: BrowserScenario) {
    this.scenario = scenario
    this.#reviewContext =
      scenario.id === BrowserScenarioId.MainReviewCloze
        ? createClozeReviewContext()
        : scenario.id === BrowserScenarioId.MainReviewCodeBlock
          ? createCodeBlockReviewContext()
        : createReviewContext()
    if (scenario.id === BrowserScenarioId.MainBrowseBasic) {
      this.#insertBasicCard(
        'Why does retrieval practice work?',
        'It strengthens the route used to recall the memory.',
        'Testing notes',
      )
    }
    if (scenario.id === BrowserScenarioId.MainBrowseDeepRoute) {
      for (let sequence = 1; sequence <= 55; sequence += 1) {
        this.#insertBasicCard(
          `Deep route card ${sequence}`,
          `Deep route answer ${sequence}`,
          'Navigation testing',
        )
      }
    }
    if (scenario.id === BrowserScenarioId.MainBrowseHistory) {
      for (const number of ['one', 'two', 'three', 'four', 'five']) {
        this.#insertBasicCard(
          `History card ${number}`,
          `History answer ${number}`,
          'Navigation testing',
        )
      }
    }
    if (scenario.id === BrowserScenarioId.QuickAddCreateFailsOnce) {
      this.#remainingCreateFailures = 1
    }
  }

  async invoke(commandValue: string, payload: unknown): Promise<unknown> {
    if (!ipcCommands.has(commandValue as DaraIpcCommandType)) {
      throw new Error(`Unknown Dara IPC command: ${commandValue}`)
    }
    const command = commandValue as DaraIpcCommandType
    this.#commands.push({
      command,
      payload: recordedPayload(command, payload),
    })
    switch (command) {
      case DaraIpcCommand.LoadApplicationLaunchContext:
        requireEmptyPayload(payload, command)
        return { mode: ApplicationLaunchMode.Normal }
      case DaraIpcCommand.CreateCardContent: {
        const envelope = requireRecord(payload, command)
        const content = requireCardContentDraft(envelope.input, command)
        const mediaLeaseId = requireString(
          envelope.mediaLeaseId,
          'mediaLeaseId',
          command,
        )
        if (this.#remainingCreateFailures > 0) {
          this.#remainingCreateFailures -= 1
          throw new Error('The deterministic save fault fired.')
        }
        this.#cards.push({ content: structuredClone(content), mediaLeaseId })
        const item = this.#insertBasicCard(
          content.frontMd,
          content.backMd,
          content.source,
        )
        return createReviewContext(item)
      }
      case DaraIpcCommand.DismissQuickAdd:
        requireEmptyPayload(payload, command)
        this.#dismissedQuickAdd += 1
        return null
      case DaraIpcCommand.OpenExternalUrl: {
        const envelope = requireRecord(payload, command)
        requireString(envelope.url, 'url', command)
        return null
      }
      case DaraIpcCommand.LoadHomeStats:
        requireRecord(payload, command)
        return {
          activity: [
            { studyDay: 20_287, count: 3 },
            { studyDay: 20_288, count: 7 },
          ],
          reviewedToday: this.#recordedGrades.length,
          queue: {
            learning: 0,
            new: this.#recordedGrades.length === 0 ? 1 : 0,
            review: 0,
          },
          nextLearningDueAt: null,
        }
      case DaraIpcCommand.LoadCardContent: {
        const envelope = requireRecord(payload, command)
        const cardContentId = requireString(
          envelope.cardContentId,
          'cardContentId',
          command,
        )
        return structuredClone(this.#requireItem(cardContentId, command))
      }
      case DaraIpcCommand.LoadSettings:
        requireEmptyPayload(payload, command)
        return {
          appearance: Appearance.System,
          automaticUpdateChecksEnabled: true,
          desiredRetention: 0.9,
          keyboardBindings: DEFAULT_KEYBOARD_BINDINGS,
          launchAtLogin: false,
          launchAtLoginError: null,
          legacyZoomMigrated: true,
          revision: 1,
          shortcutErrors: [],
          zoomPercent: 100,
        }
      case DaraIpcCommand.LoadOffsiteBackupStatus:
        requireEmptyPayload(payload, command)
        return structuredClone(this.#offsiteBackupStatus)
      case DaraIpcCommand.LoadRestoredOffsiteBackupTakeoverRequired:
        requireEmptyPayload(payload, command)
        return this.#restoredBackupTakeoverRequired
      case DaraIpcCommand.TestAndEnableOffsiteBackup:
      case DaraIpcCommand.ChangeOffsiteBackupTarget: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        const target = requireRecord(input.target, command)
        requireRecord(input.credentials, command)
        this.#offsiteBackupStatus = enabledBackupStatus({
          accountId: requireString(
            target.accountId,
            'input.target.accountId',
            command,
          ),
          jurisdiction: requireJurisdiction(
            target.jurisdiction,
            command,
          ),
          bucket: requireString(target.bucket, 'input.target.bucket', command),
        })
        return backupOperation(
          command === DaraIpcCommand.ChangeOffsiteBackupTarget
            ? OffsiteBackupOperationKind.ChangeTarget
            : OffsiteBackupOperationKind.TestAndEnable,
        )
      }
      case DaraIpcCommand.ReplaceOffsiteBackupCredentials: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        requireRecord(input.credentials, command)
        this.#offsiteBackupStatus.credentials =
          CredentialAvailability.Present
        return backupOperation(OffsiteBackupOperationKind.ReplaceCredentials)
      }
      case DaraIpcCommand.CreateOffsiteBackupNow:
        requireEmptyPayload(payload, command)
        this.#offsiteBackupStatus.checkpoint = completeCheckpointStatus()
        return backupOperation(OffsiteBackupOperationKind.BackupNow)
      case DaraIpcCommand.RunOffsiteRestoreDrill:
        requireEmptyPayload(payload, command)
        this.#offsiteBackupStatus.lastRestoreDrill = successfulDrillReport()
        this.#offsiteBackupStatus.lastRestoreDrillAt = 1_788_512_500_000
        return backupOperation(OffsiteBackupOperationKind.RestoreDrill)
      case DaraIpcCommand.DisableOffsiteBackup:
        requireEmptyPayload(payload, command)
        this.#offsiteBackupStatus.enabled = false
        this.#offsiteBackupStatus.relational.phase =
          RelationalBackupPhase.Off
        this.#offsiteBackupStatus.media.phase = MediaBackupPhase.Off
        this.#offsiteBackupStatus.checkpoint.phase =
          CheckpointBackupPhase.Off
        return backupOperation(OffsiteBackupOperationKind.Disable)
      case DaraIpcCommand.RemoveOffsiteBackupCredentials:
        requireEmptyPayload(payload, command)
        this.#offsiteBackupStatus.enabled = false
        this.#offsiteBackupStatus.credentials =
          CredentialAvailability.Missing
        return backupOperation(OffsiteBackupOperationKind.RemoveCredentials)
      case DaraIpcCommand.TakeOverRestoredOffsiteBackup: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        if (input.confirmed !== true) {
          throw malformed(command, 'input.confirmed must be true')
        }
        this.#offsiteBackupStatus.takeoverAvailable = false
        this.#restoredBackupTakeoverRequired = false
        return backupOperation(OffsiteBackupOperationKind.TakeOver)
      }
      case DaraIpcCommand.LoadDiagnostics:
        requireEmptyPayload(payload, command)
        return diagnosticsSnapshot(this.#items.length)
      case DaraIpcCommand.RecordGrade: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(
          envelope.input,
          command,
        ) as unknown as RecordGradeInput
        this.#recordedGrades.push(structuredClone(input))
        this.#reviewContext.cache = structuredClone(input.nextCache)
        this.#reviewContext.lastCardSequence = input.expectedCardSequence + 1
        this.#reviewContext.reviewCard.updatedAt += 1
        this.#reviewContext.reviewHistory.push({
          cardSequence: this.#reviewContext.lastCardSequence,
          eventId: input.eventId,
          review: structuredClone(input.review),
          schedulerConfigId: input.expectedSchedulerConfigId,
          schedulerLog: structuredClone(input.schedulerLog),
        })
        return {
          disposition: MutationDisposition.Applied,
          eventId: input.eventId,
          cardSequence: this.#reviewContext.lastCardSequence,
          context: this.#reviewContext,
        }
      }
      case DaraIpcCommand.UndoLastGrade: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        const eventId = requireString(input.eventId, 'input.eventId', command)
        requireString(input.targetEventId, 'input.targetEventId', command)
        this.#recordedGrades.pop()
        this.#reviewContext.reviewHistory.pop()
        this.#reviewContext.cache = structuredClone(
          requireRecord(input.nextCache, command),
        ) as unknown as ReviewContext['cache']
        this.#reviewContext.lastCardSequence += 1
        this.#reviewContext.reviewCard.updatedAt += 1
        return {
          disposition: MutationDisposition.Applied,
          eventId,
          cardSequence: this.#reviewContext.lastCardSequence,
          context: this.#reviewContext,
        }
      }
      case DaraIpcCommand.RenewMediaLease: {
        const envelope = requireRecord(payload, command)
        requireString(envelope.leaseId, 'leaseId', command)
        return 1_788_512_400_000
      }
      case DaraIpcCommand.SearchCardContent: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        const query = requireString(input.query, 'input.query', command)
        requireNonNegativeInteger(input.offset, 'input.offset', command)
        requirePositiveInteger(input.limit, 'input.limit', command)
        const normalizedQuery = query.trim().toLocaleLowerCase()
        const matching = this.#items.filter((item) =>
          normalizedQuery === '' ||
          [
            item.cardContent.frontMd,
            item.cardContent.backMd,
            item.cardContent.source ?? '',
          ].some((value) => value.toLocaleLowerCase().includes(normalizedQuery)),
        )
        return {
          items: structuredClone(
            matching.slice(
              input.offset as number,
              (input.offset as number) + (input.limit as number),
            ),
          ),
          mode: query === '' ? SearchExecutionMode.Browse : SearchExecutionMode.Hybrid,
          semanticStatus: semanticReadyStatus(),
        }
      }
      case DaraIpcCommand.SearchStatus:
        requireEmptyPayload(payload, command)
        return semanticReadyStatus()
      case DaraIpcCommand.SelectNextReviewCard:
        requireRecord(payload, command)
        return this.#recordedGrades.length === 0
          ? {
              context: this.#reviewContext,
              kind: ReviewQueueSelectionKind.Card,
              lane: ReviewQueueLane.New,
              nextNormalLaneCursor: 0,
            }
          : {
              kind: ReviewQueueSelectionKind.CaughtUp,
              nextDueAt: null,
              nextNormalLaneCursor: 0,
            }
      case DaraIpcCommand.SetQuickAddFileDialogOpen: {
        const envelope = requireRecord(payload, command)
        if (typeof envelope.open !== 'boolean') {
          throw malformed(command, 'open must be a boolean')
        }
        return null
      }
      case DaraIpcCommand.UpdateCardContent: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        const id = requireString(input.id, 'input.id', command)
        requireNonNegativeInteger(
          input.expectedUpdatedAt,
          'input.expectedUpdatedAt',
          command,
        )
        requireString(envelope.mediaLeaseId, 'mediaLeaseId', command)
        const content = requireCardContentDraft(input.content, command)
        const item = this.#requireItem(id, command)
        const updatedAt = item.cardContent.updatedAt + 1
        item.cardContent = {
          ...item.cardContent,
          ...structuredClone(content),
          updatedAt,
        }
        item.lifecycleUpdatedAt = updatedAt
        return structuredClone(item)
      }
      case DaraIpcCommand.SetCardContentSuspended: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        const item = this.#requireItem(
          requireString(input.cardContentId, 'input.cardContentId', command),
          command,
        )
        requireNonNegativeInteger(
          input.expectedLifecycleUpdatedAt,
          'input.expectedLifecycleUpdatedAt',
          command,
        )
        if (typeof input.suspended !== 'boolean') {
          throw malformed(command, 'input.suspended must be a boolean')
        }
        const status = input.suspended
          ? ReviewCardStatus.Suspended
          : ReviewCardStatus.Active
        item.reviewCards = item.reviewCards.map((reviewCard) => ({
          ...reviewCard,
          status,
        }))
        item.reviewStatus = input.suspended
          ? CardContentReviewStatus.Suspended
          : CardContentReviewStatus.Active
        item.lifecycleUpdatedAt += 1
        return structuredClone(item)
      }
      case DaraIpcCommand.DeleteCardContent: {
        const envelope = requireRecord(payload, command)
        const input = requireRecord(envelope.input, command)
        const id = requireString(
          input.cardContentId,
          'input.cardContentId',
          command,
        )
        requireNonNegativeInteger(
          input.expectedUpdatedAt,
          'input.expectedUpdatedAt',
          command,
        )
        requireNonNegativeInteger(
          input.expectedLifecycleUpdatedAt,
          'input.expectedLifecycleUpdatedAt',
          command,
        )
        const index = this.#items.findIndex((item) => item.cardContent.id === id)
        if (index === -1) {
          throw malformed(command, `unknown card content id ${id}`)
        }
        this.#items.splice(index, 1)
        return null
      }
      default:
        throw new Error(`FakeDaraBackend does not implement ${command}`)
    }
  }

  snapshot(): FakeBackendSnapshot {
    return {
      cards: structuredClone(this.#cards),
      commands: structuredClone(this.#commands),
      dismissedQuickAdd: this.#dismissedQuickAdd,
      recordedGrades: structuredClone(this.#recordedGrades),
    }
  }

  #insertBasicCard(
    frontMd: string,
    backMd: string,
    source: string | null,
  ): CardContentListItem {
    const sequence = this.#items.length + 1
    const timestamp = 1_752_768_000_000 + sequence
    const suffix = sequence.toString(16).padStart(12, '0')
    const item: CardContentListItem = {
      cardContent: {
        backMd,
        createdAt: timestamp,
        frontMd,
        id: `01980c8e-6c00-7000-8000-${suffix}`,
        source,
        type: CardContentType.Basic,
        updatedAt: timestamp,
      },
      lifecycleUpdatedAt: timestamp,
      reviewCards: [
        {
          dueAt: null,
          dueStudyDay: null,
          id: `01980c8e-6c00-7001-8000-${suffix}`,
          lastReviewAt: null,
          state: ReviewCardState.New,
          status: ReviewCardStatus.Active,
          variantKey: 'basic',
        },
      ],
      reviewStatus: CardContentReviewStatus.Active,
    }
    this.#items.unshift(item)
    return item
  }

  #requireItem(
    id: string,
    command: DaraIpcCommandType,
  ): CardContentListItem {
    const item = this.#items.find((candidate) => candidate.cardContent.id === id)
    if (!item) {
      throw malformed(command, `unknown card content id ${id}`)
    }
    return item
  }
}

function createClozeReviewContext(): ReviewContext {
  const context = createReviewContext()
  return {
    ...context,
    cardContent: {
      backMd: '',
      createdAt: context.cardContent.createdAt,
      frontMd: [
        'I am in Neovim, loaded with my *personal* setup. I type `gd` over **a Python function**. This is what happens:',
        '',
        '1. Your buffer-local map fires {{c1::`snacks.picker.lsp_definitions()`::a picker plugin that handles several editor quality-of-life features}}.',
        '2. {{c1::The picker builds a position and hands it to Neovim’s built-in LSP client::the picker integration}}.',
        '3. `vim.lsp` sends `textDocument/definition` to the running language server.',
      ].join('\n'),
      id: context.cardContent.id,
      source: null,
      type: CardContentType.Cloze,
      updatedAt: context.cardContent.updatedAt,
    },
    reviewCard: {
      ...context.reviewCard,
      variantKey: 'cloze:1',
    },
  }
}

function createCodeBlockReviewContext(): ReviewContext {
  const context = createReviewContext()
  return {
    ...context,
    cardContent: {
      ...context.cardContent,
      backMd: '',
      frontMd: [
        'Compare inline `main` with highlighted and plain code blocks:',
        '',
        '```python',
        'def main():',
        '    ...',
        '```',
        '',
        '```',
        'def main():',
        '    hello',
        '```',
      ].join('\n'),
      source: null,
    },
  }
}

function createReviewContext(item?: CardContentListItem): ReviewContext {
  const now = 1_752_768_000_000
  return {
    cache: createNewReviewCardCache(),
    cacheSchedulerConfigId: null,
    cardContent: item?.cardContent ?? {
      backMd: [
        '`?1` — **48**',
        '',
        '`?2` — **0x30**',
        '',
        '***Both forms return*** `0`.',
      ].join('\n'),
      createdAt: now,
      frontMd: [
        'Given this Unicode code point: `U+0030`, replace `?1` and `?2` with the two values I *could* use to get the corresponding character in Python:',
        '',
        '- `chr(?1)`',
        '- `chr(?2)`',
      ].join('\n'),
      id: '01980c8e-6c00-7000-8000-000000000201',
      source: 'Python · Unicode',
      type: CardContentType.Basic,
      updatedAt: now,
    },
    lastCardSequence: 0,
    reviewCard: {
      id: '01980c8e-6c00-7000-8000-000000000202',
      status: ReviewCardStatus.Active,
      updatedAt: now,
      variantKey: 'basic',
    },
    reviewHistory: [],
    schedulerConfig: {
      ...DEFAULT_SCHEDULER_CONFIG,
      id: '01980c8e-6c00-7000-8000-000000000203',
    },
  }
}

function semanticReadyStatus() {
  return {
    downloadedBytes: 232_883_776,
    indexedDocuments: 1,
    message: null,
    modelBytes: 232_883_776,
    phase: SemanticSearchPhase.Ready,
    totalDocuments: 1,
  }
}

function diagnosticsSnapshot(totalDocuments: number) {
  const semanticStatus = {
    ...semanticReadyStatus(),
    indexedDocuments: totalDocuments,
    totalDocuments,
  }
  return {
    applicationVersion: '0.1.0',
    database: {
      migrationHeads: { main: 7, media: 4 },
      scheduler: {
        algorithm: DEFAULT_SCHEDULER_CONFIG.algorithm,
        algorithmVersion: DEFAULT_SCHEDULER_CONFIG.algorithmVersion,
        schedulerLibrary: DEFAULT_SCHEDULER_CONFIG.schedulerLibrary,
        libraryVersion: DEFAULT_SCHEDULER_CONFIG.libraryVersion,
        desiredRetention: DEFAULT_SCHEDULER_CONFIG.config.desiredRetention,
      },
      semanticIndex: {
        id: 'jina_v1',
        active: true,
        indexedDocuments: totalDocuments,
        totalDocuments,
      },
    },
    semanticModel: {
      modelName: 'jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF',
      phase: semanticStatus.phase,
      downloadedBytes: semanticStatus.downloadedBytes,
      expectedBytes: semanticStatus.modelBytes,
      lastError: null,
    },
    storage: {
      relationalDatabaseBytes: 1_048_576,
      mediaDatabaseBytes: 524_288,
      modelBytes: semanticStatus.modelBytes,
      snapshotsBytes: 1_572_864,
      logsBytes: 4_096,
    },
    latestSnapshot: {
      createdAt: 1_788_512_400_000,
      applicationVersion: '0.1.0',
    },
    lastMediaMaintenance: {
      inspectedAt: 1_788_512_400_000,
      integrity: {
        orphanedImageIds: [],
        extraBlobSha256: [],
        missingReferencedBlobImageIds: [],
        extraBlobBytes: 0,
      },
      cleanup: {
        retiredImageCount: 0,
        deletedBlobCount: 0,
        reclaimedBytes: 0,
      },
    },
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
    restoredTakeoverRequired: false,
    credentialCleanupPending: false,
    activeOperation: null,
  }
}

function enabledBackupStatus(
  target: NonNullable<OffsiteBackupStatus['target']>,
): OffsiteBackupStatus {
  return {
    ...disabledBackupStatus(),
    configured: true,
    enabled: true,
    revision: 1,
    target,
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
      verifiedCount: 3,
      verifiedBytes: 2048,
      blockedCount: 0,
      lastErrorCode: null,
    },
    checkpoint: {
      phase: CheckpointBackupPhase.WaitingForMedia,
      inProgressCheckpointId: null,
      lastCompleteCheckpointId: null,
      lastCompleteAt: null,
      lastErrorCode: null,
    },
  }
}

function completeCheckpointStatus(): OffsiteBackupStatus['checkpoint'] {
  return {
    phase: CheckpointBackupPhase.Idle,
    inProgressCheckpointId: null,
    lastCompleteCheckpointId:
      '019f547b-6200-7000-8000-000000000001',
    lastCompleteAt: 1_788_512_400_000,
    lastErrorCode: null,
  }
}

function successfulDrillReport(): NonNullable<
  OffsiteBackupStatus['lastRestoreDrill']
> {
  return {
    formatVersion: 2,
    backupSetId: '019f547b-6200-7000-8000-000000000010',
    replicaEpochId: '019f547b-6200-7000-8000-000000000011',
    outcome: RestoreDrillOutcome.Success,
    checkpointId: '019f547b-6200-7000-8000-000000000001',
    checkpointCreatedAt: '2026-09-03T12:00:00Z',
    restoredTxid: '000000000000000a',
    mainMigrationHead: 9,
    mediaMigrationHead: 4,
    referencedMediaCount: 3,
    referencedMediaBytes: 2048,
    validationStages: [],
    durationMs: 4321,
    daraVersion: '0.1.0',
    errorCode: null,
  }
}

function backupOperation(operation: OffsiteBackupOperationKind) {
  return {
    operationId: '019f547b-6200-7000-8000-000000000099',
    operation,
    reused: false,
  }
}

function requireJurisdiction(
  value: unknown,
  command: DaraIpcCommandType,
): R2Jurisdiction {
  if (
    value !== R2Jurisdiction.Default &&
    value !== R2Jurisdiction.Eu &&
    value !== R2Jurisdiction.Fedramp
  ) {
    throw malformed(command, 'input.target.jurisdiction is invalid')
  }
  return value
}

function recordedPayload(
  command: DaraIpcCommandType,
  payload: unknown,
): unknown {
  if (
    command !== DaraIpcCommand.TestAndEnableOffsiteBackup &&
    command !== DaraIpcCommand.ChangeOffsiteBackupTarget &&
    command !== DaraIpcCommand.ReplaceOffsiteBackupCredentials
  ) {
    return structuredClone(payload)
  }
  const clone = structuredClone(payload)
  const envelope = requireRecord(clone, command)
  const input = requireRecord(envelope.input, command)
  if ('credentials' in input) {
    input.credentials = '[REDACTED]'
  }
  return clone
}

function requireCardContentDraft(
  value: unknown,
  command: DaraIpcCommandType,
): BasicCardContentDraft {
  const content = requireRecord(value, command)
  if (content.type !== CardContentType.Basic) {
    throw malformed(command, 'the initial browser slice accepts a Basic card')
  }
  requireString(content.frontMd, 'input.frontMd', command)
  requireString(content.backMd, 'input.backMd', command)
  if (content.source !== null && typeof content.source !== 'string') {
    throw malformed(command, 'input.source must be a string or null')
  }
  return content as unknown as BasicCardContentDraft
}

function requireRecord(
  value: unknown,
  command: DaraIpcCommandType,
): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw malformed(command, 'payload must be an object')
  }
  return value as Record<string, unknown>
}

function requireString(
  value: unknown,
  field: string,
  command: DaraIpcCommandType,
): string {
  if (typeof value !== 'string') {
    throw malformed(command, `${field} must be a string`)
  }
  return value
}

function requireNonNegativeInteger(
  value: unknown,
  field: string,
  command: DaraIpcCommandType,
): number {
  if (!Number.isInteger(value) || (value as number) < 0) {
    throw malformed(command, `${field} must be a non-negative integer`)
  }
  return value as number
}

function requirePositiveInteger(
  value: unknown,
  field: string,
  command: DaraIpcCommandType,
): number {
  const integer = requireNonNegativeInteger(value, field, command)
  if (integer === 0) {
    throw malformed(command, `${field} must be positive`)
  }
  return integer
}

function requireEmptyPayload(value: unknown, command: DaraIpcCommandType): void {
  if (Object.keys(requireRecord(value, command)).length !== 0) {
    throw malformed(command, 'payload must be empty')
  }
}

function malformed(command: DaraIpcCommandType, detail: string): Error {
  return new Error(`Malformed ${command} payload: ${detail}`)
}
