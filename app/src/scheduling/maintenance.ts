import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
import { parseSchedulerConfig } from './config.ts'
import { replayReviews } from './scheduler.ts'
import type {
  ReviewCardCache,
  ReviewFact,
  SchedulerConfigRecord,
} from './types.ts'
import { SchedulingError } from './types.ts'

export const SchedulerReplayInstallOperation = {
  Repair: 'REPAIR',
  ActivateConfig: 'ACTIVATE_CONFIG',
} as const

export type SchedulerReplayInstallOperation =
  (typeof SchedulerReplayInstallOperation)[keyof typeof SchedulerReplayInstallOperation]

export const SchedulerMaintenanceOperation = {
  Check: 'CHECK',
  Repair: 'REPAIR',
  ChangeDesiredRetention: 'CHANGE_DESIRED_RETENTION',
} as const

export type SchedulerMaintenanceOperation =
  (typeof SchedulerMaintenanceOperation)[keyof typeof SchedulerMaintenanceOperation]

export const SchedulerReplayDifferenceKind = {
  InvalidStoredCache: 'INVALID_STORED_CACHE',
  SchedulerConfig: 'SCHEDULER_CONFIG',
  Cache: 'CACHE',
} as const

export type SchedulerReplayDifferenceKind =
  (typeof SchedulerReplayDifferenceKind)[keyof typeof SchedulerReplayDifferenceKind]

const MIN_USER_DESIRED_RETENTION = 0.70
const MAX_USER_DESIRED_RETENTION = 0.99

export interface SchedulerReplayCard {
  reviewCardId: string
  expectedUpdatedAt: number
  expectedCardSequence: number
  expectedSchedulerConfigId: string
  storedCache: ReviewCardCache | null
  storedCacheError: string | null
  reviewHistory: ReviewFact[]
}

export interface SchedulerReplaySnapshot {
  sourceActiveSchedulerConfigId: string
  targetSchedulerConfig: SchedulerConfigRecord
  targetIsNew: boolean
  cards: SchedulerReplayCard[]
}

export interface StagedSchedulerReplayCard {
  reviewCardId: string
  expectedUpdatedAt: number
  expectedCardSequence: number
  expectedSchedulerConfigId: string
  install: boolean
  cache: ReviewCardCache
}

export interface InstallSchedulerReplayInput {
  operation: SchedulerReplayInstallOperation
  sourceActiveSchedulerConfigId: string
  targetSchedulerConfig: SchedulerConfigRecord
  cards: StagedSchedulerReplayCard[]
}

export interface SchedulerReplayInstallReport {
  operation: SchedulerReplayInstallOperation
  activeSchedulerConfigId: string
  evaluatedCards: number
  installedCards: number
}

export interface SchedulerReplayDifference {
  reviewCardId: string
  kind: SchedulerReplayDifferenceKind
  detail: string | null
}

export interface CalculatedSchedulerReplay {
  snapshot: SchedulerReplaySnapshot
  cards: StagedSchedulerReplayCard[]
  differences: SchedulerReplayDifference[]
}

export interface SchedulerMaintenanceReport {
  operation: SchedulerMaintenanceOperation
  sourceSchedulerConfigId: string
  activeSchedulerConfigId: string
  desiredRetention: number
  evaluatedCards: number
  differingCards: number
  installedCards: number
  differences: SchedulerReplayDifference[]
}

export interface SchedulerMaintenanceGateway {
  loadSchedulerReplaySnapshot(): Promise<SchedulerReplaySnapshot>
  prepareDesiredRetentionReplay(
    desiredRetention: number,
  ): Promise<SchedulerReplaySnapshot>
  installSchedulerReplay(
    input: InstallSchedulerReplayInput,
  ): Promise<SchedulerReplayInstallReport>
}

export interface SchedulerRecalculationProgress {
  completedCards: number
  totalCards: number
}

export interface SchedulerRecalculationOptions {
  onBeforeInstall?: () => void
  onProgress?: (progress: SchedulerRecalculationProgress) => void
  signal?: AbortSignal
}

const REPLAY_YIELD_INTERVAL = 100

export const tauriSchedulerMaintenanceGateway: SchedulerMaintenanceGateway = {
  loadSchedulerReplaySnapshot: () =>
    invoke<SchedulerReplaySnapshot>(DaraIpcCommand.LoadSchedulerReplaySnapshot),
  prepareDesiredRetentionReplay: (desiredRetention: number) =>
    invoke<SchedulerReplaySnapshot>(DaraIpcCommand.PrepareDesiredRetentionReplay, {
      input: { desiredRetention },
    }),
  installSchedulerReplay: (input: InstallSchedulerReplayInput) =>
    invoke<SchedulerReplayInstallReport>(DaraIpcCommand.InstallSchedulerReplay, { input }),
}

export async function checkSchedulingData(
  gateway: SchedulerMaintenanceGateway = tauriSchedulerMaintenanceGateway,
): Promise<SchedulerMaintenanceReport> {
  const snapshot = await gateway.loadSchedulerReplaySnapshot()
  requireTargetKind(snapshot, false)
  const calculated = calculateSchedulerReplay(snapshot, false)
  return maintenanceReport(
    SchedulerMaintenanceOperation.Check,
    calculated,
    snapshot.sourceActiveSchedulerConfigId,
    0,
  )
}

export async function repairSchedulingData(
  gateway: SchedulerMaintenanceGateway = tauriSchedulerMaintenanceGateway,
): Promise<SchedulerMaintenanceReport> {
  const snapshot = await gateway.loadSchedulerReplaySnapshot()
  requireTargetKind(snapshot, false)
  const calculated = calculateSchedulerReplay(snapshot, false)
  if (calculated.differences.length === 0) {
    return maintenanceReport(
      SchedulerMaintenanceOperation.Repair,
      calculated,
      snapshot.sourceActiveSchedulerConfigId,
      0,
    )
  }

  const installed = await gateway.installSchedulerReplay({
    operation: SchedulerReplayInstallOperation.Repair,
    sourceActiveSchedulerConfigId: snapshot.sourceActiveSchedulerConfigId,
    targetSchedulerConfig: snapshot.targetSchedulerConfig,
    cards: calculated.cards,
  })
  return maintenanceReport(
    SchedulerMaintenanceOperation.Repair,
    calculated,
    installed.activeSchedulerConfigId,
    installed.installedCards,
  )
}

export async function changeDesiredRetention(
  desiredRetention: number,
  gateway: SchedulerMaintenanceGateway = tauriSchedulerMaintenanceGateway,
  options: SchedulerRecalculationOptions = {},
): Promise<SchedulerMaintenanceReport> {
  validateUserDesiredRetention(desiredRetention)
  const snapshot = await gateway.prepareDesiredRetentionReplay(desiredRetention)
  requireTargetKind(snapshot, true)
  const calculated = await calculateSchedulerReplayWithProgress(
    snapshot,
    true,
    options,
  )
  throwIfCancelled(options.signal)
  options.onBeforeInstall?.()
  const installed = await gateway.installSchedulerReplay({
    operation: SchedulerReplayInstallOperation.ActivateConfig,
    sourceActiveSchedulerConfigId: snapshot.sourceActiveSchedulerConfigId,
    targetSchedulerConfig: snapshot.targetSchedulerConfig,
    cards: calculated.cards,
  })
  return maintenanceReport(
    SchedulerMaintenanceOperation.ChangeDesiredRetention,
    calculated,
    installed.activeSchedulerConfigId,
    installed.installedCards,
  )
}

async function calculateSchedulerReplayWithProgress(
  snapshot: SchedulerReplaySnapshot,
  installEveryCard: boolean,
  options: SchedulerRecalculationOptions,
): Promise<CalculatedSchedulerReplay> {
  const config = parseSchedulerConfig(snapshot.targetSchedulerConfig)
  const differences: SchedulerReplayDifference[] = []
  const cards: StagedSchedulerReplayCard[] = []
  options.onProgress?.({ completedCards: 0, totalCards: snapshot.cards.length })
  for (let index = 0; index < snapshot.cards.length; index += 1) {
    throwIfCancelled(options.signal)
    const card = snapshot.cards[index]!
    const cache = replayReviews(
      card.reviewCardId,
      card.reviewHistory,
      config,
    ).cache
    const cardDifferences = replayDifferences(
      card,
      cache,
      snapshot.targetSchedulerConfig.id,
    )
    differences.push(...cardDifferences)
    cards.push({
      reviewCardId: card.reviewCardId,
      expectedUpdatedAt: card.expectedUpdatedAt,
      expectedCardSequence: card.expectedCardSequence,
      expectedSchedulerConfigId: card.expectedSchedulerConfigId,
      install: installEveryCard || cardDifferences.length > 0,
      cache,
    })
    const completedCards = index + 1
    options.onProgress?.({
      completedCards,
      totalCards: snapshot.cards.length,
    })
    if (
      completedCards < snapshot.cards.length &&
      completedCards % REPLAY_YIELD_INTERVAL === 0
    ) {
      await yieldToWindow()
    }
  }
  return { snapshot, cards, differences }
}

function throwIfCancelled(signal: AbortSignal | undefined): void {
  if (signal?.aborted) {
    throw new DOMException('Schedule update cancelled', 'AbortError')
  }
}

function yieldToWindow(): Promise<void> {
  return new Promise((resolve) => globalThis.setTimeout(resolve, 0))
}

export function calculateSchedulerReplay(
  snapshot: SchedulerReplaySnapshot,
  installEveryCard: boolean,
): CalculatedSchedulerReplay {
  const config = parseSchedulerConfig(snapshot.targetSchedulerConfig)
  const differences: SchedulerReplayDifference[] = []
  const cards = snapshot.cards.map((card) => {
    const cache = replayReviews(
      card.reviewCardId,
      card.reviewHistory,
      config,
    ).cache
    const cardDifferences = replayDifferences(
      card,
      cache,
      snapshot.targetSchedulerConfig.id,
    )
    differences.push(...cardDifferences)
    return {
      reviewCardId: card.reviewCardId,
      expectedUpdatedAt: card.expectedUpdatedAt,
      expectedCardSequence: card.expectedCardSequence,
      expectedSchedulerConfigId: card.expectedSchedulerConfigId,
      install: installEveryCard || cardDifferences.length > 0,
      cache,
    }
  })
  return { snapshot, cards, differences }
}

function replayDifferences(
  card: SchedulerReplayCard,
  replayed: ReviewCardCache,
  targetSchedulerConfigId: string,
): SchedulerReplayDifference[] {
  const differences: SchedulerReplayDifference[] = []
  if (card.storedCache === null) {
    differences.push({
      reviewCardId: card.reviewCardId,
      kind: SchedulerReplayDifferenceKind.InvalidStoredCache,
      detail: card.storedCacheError,
    })
  } else if (!cacheEquals(card.storedCache, replayed)) {
    differences.push({
      reviewCardId: card.reviewCardId,
      kind: SchedulerReplayDifferenceKind.Cache,
      detail: null,
    })
  }
  if (card.expectedSchedulerConfigId !== targetSchedulerConfigId) {
    differences.push({
      reviewCardId: card.reviewCardId,
      kind: SchedulerReplayDifferenceKind.SchedulerConfig,
      detail: card.expectedSchedulerConfigId,
    })
  }
  return differences
}

function cacheEquals(left: ReviewCardCache, right: ReviewCardCache): boolean {
  return (
    left.state === right.state &&
    left.dueAt === right.dueAt &&
    left.dueStudyDay === right.dueStudyDay &&
    left.lastReviewAt === right.lastReviewAt &&
    left.reps === right.reps &&
    left.lapses === right.lapses &&
    schedulerStateEquals(left.schedulerState, right.schedulerState)
  )
}

function schedulerStateEquals(
  left: ReviewCardCache['schedulerState'],
  right: ReviewCardCache['schedulerState'],
): boolean {
  if (left === null || right === null) {
    return left === right
  }
  return (
    left.stability === right.stability &&
    left.difficulty === right.difficulty &&
    left.scheduledDays === right.scheduledDays &&
    left.learningSteps === right.learningSteps
  )
}

function maintenanceReport(
  operation: SchedulerMaintenanceOperation,
  calculated: CalculatedSchedulerReplay,
  activeSchedulerConfigId: string,
  installedCards: number,
): SchedulerMaintenanceReport {
  const differingCards = new Set(
    calculated.differences.map((difference) => difference.reviewCardId),
  ).size
  return {
    operation,
    sourceSchedulerConfigId:
      calculated.snapshot.sourceActiveSchedulerConfigId,
    activeSchedulerConfigId,
    desiredRetention: parseSchedulerConfig(
      calculated.snapshot.targetSchedulerConfig,
    ).config.desiredRetention,
    evaluatedCards: calculated.cards.length,
    differingCards,
    installedCards,
    differences: calculated.differences,
  }
}

function requireTargetKind(
  snapshot: SchedulerReplaySnapshot,
  targetIsNew: boolean,
): void {
  if (snapshot.targetIsNew !== targetIsNew) {
    throw new SchedulingError(
      targetIsNew
        ? 'the desired-retention replay did not produce a new scheduler config'
        : 'the scheduling-data replay unexpectedly produced a new scheduler config',
    )
  }
}

function validateUserDesiredRetention(value: number): void {
  if (
    !Number.isFinite(value) ||
    value < MIN_USER_DESIRED_RETENTION ||
    value > MAX_USER_DESIRED_RETENTION ||
    !Number.isInteger(value * 100)
  ) {
    throw new SchedulingError(
      `desired retention must be a whole percentage between ${MIN_USER_DESIRED_RETENTION} and ${MAX_USER_DESIRED_RETENTION}`,
    )
  }
}
