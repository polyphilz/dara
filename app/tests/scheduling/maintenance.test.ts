import assert from 'node:assert/strict'
import test from 'node:test'
import {
  DEFAULT_SCHEDULER_CONFIG,
  SchedulerMaintenanceOperation,
  SchedulerReplayDifferenceKind,
  SchedulerReplayInstallOperation,
  SchedulingError,
  calculateSchedulerReplay,
  changeDesiredRetention,
  checkSchedulingData,
  repairSchedulingData,
  replayReviews,
} from '../../src/scheduling/index.ts'
import type {
  InstallSchedulerReplayInput,
  ReviewFact,
  SchedulerConfigRecord,
  SchedulerMaintenanceGateway,
  SchedulerReplayInstallReport,
  SchedulerReplaySnapshot,
} from '../../src/scheduling/index.ts'

const ACTIVE_CONFIG_ID = '019f547b-6200-7000-8000-000000000001'
const TARGET_CONFIG_ID = '019f547b-6200-7000-8000-000000000002'
const CARD_A = '01980c8e-6c00-7000-8000-000000000101'
const CARD_B = '01980c8e-6c00-7000-8000-000000000102'

const review: ReviewFact = {
  reviewedAt: 1_783_929_300_000,
  studyDay: 20_646,
  timezoneId: 'America/New_York',
  utcOffsetMinutes: -240,
  grade: 3,
}

test('checks every replayed cache without writing when scheduling data matches', async () => {
  const snapshot = activeSnapshot()
  const gateway = new FakeGateway(snapshot)

  const report = await checkSchedulingData(gateway)

  assert.equal(report.operation, SchedulerMaintenanceOperation.Check)
  assert.equal(report.evaluatedCards, 2)
  assert.equal(report.differingCards, 0)
  assert.equal(report.installedCards, 0)
  assert.deepEqual(report.differences, [])
  assert.deepEqual(gateway.installInputs, [])
})

test('reports invalid, mismatched, and wrong-config caches with named reasons', () => {
  const snapshot = activeSnapshot()
  snapshot.cards[0]!.storedCache = null
  snapshot.cards[0]!.storedCacheError = 'invalid scheduler state'
  snapshot.cards[1]!.expectedSchedulerConfigId = TARGET_CONFIG_ID
  snapshot.cards[1]!.storedCache = {
    ...snapshot.cards[1]!.storedCache!,
    dueStudyDay: snapshot.cards[1]!.storedCache!.dueStudyDay! + 1,
  }

  const calculated = calculateSchedulerReplay(snapshot, false)

  assert.deepEqual(
    calculated.differences.map((difference) => difference.kind),
    [
      SchedulerReplayDifferenceKind.InvalidStoredCache,
      SchedulerReplayDifferenceKind.Cache,
      SchedulerReplayDifferenceKind.SchedulerConfig,
    ],
  )
  assert.ok(calculated.cards.every((card) => card.install))
})

test('repairs only differing caches while submitting the complete source set', async () => {
  const snapshot = activeSnapshot()
  snapshot.cards[1]!.storedCache = {
    ...snapshot.cards[1]!.storedCache!,
    dueStudyDay: snapshot.cards[1]!.storedCache!.dueStudyDay! + 1,
  }
  const gateway = new FakeGateway(snapshot)

  const report = await repairSchedulingData(gateway)

  assert.equal(report.operation, SchedulerMaintenanceOperation.Repair)
  assert.equal(report.differingCards, 1)
  assert.equal(report.installedCards, 1)
  assert.equal(gateway.installInputs.length, 1)
  assert.equal(
    gateway.installInputs[0]!.operation,
    SchedulerReplayInstallOperation.Repair,
  )
  assert.deepEqual(
    gateway.installInputs[0]!.cards.map((card) => card.install),
    [false, true],
  )
})

test('changes desired retention by replaying and installing every reviewed card', async () => {
  const active = activeSnapshot()
  const target = targetSnapshot(active, 0.85)
  const gateway = new FakeGateway(active, target)

  const report = await changeDesiredRetention(0.85, gateway)

  assert.equal(
    report.operation,
    SchedulerMaintenanceOperation.ChangeDesiredRetention,
  )
  assert.equal(report.sourceSchedulerConfigId, ACTIVE_CONFIG_ID)
  assert.equal(report.activeSchedulerConfigId, TARGET_CONFIG_ID)
  assert.equal(report.desiredRetention, 0.85)
  assert.equal(report.evaluatedCards, 2)
  assert.equal(report.installedCards, 2)
  assert.equal(gateway.preparedRetention, 0.85)
  assert.equal(gateway.installInputs.length, 1)
  const input = gateway.installInputs[0]!
  assert.equal(input.operation, SchedulerReplayInstallOperation.ActivateConfig)
  assert.equal(input.targetSchedulerConfig.id, TARGET_CONFIG_ID)
  assert.ok(input.cards.every((card) => card.install))
  assert.ok(
    input.cards.every(
      (card) => card.expectedSchedulerConfigId === ACTIVE_CONFIG_ID,
    ),
  )
})

test('can cancel a large retention replay before the atomic install', async () => {
  const active = activeSnapshot()
  active.cards = Array.from({ length: 101 }, (_, index) => ({
    ...structuredClone(active.cards[index % 2]!),
    reviewCardId: `card-${index}`,
  }))
  const target = targetSnapshot(active, 0.85)
  const gateway = new FakeGateway(active, target)
  const controller = new AbortController()

  await assert.rejects(
    () =>
      changeDesiredRetention(0.85, gateway, {
        onProgress: ({ completedCards }) => {
          if (completedCards === 100) {
            controller.abort()
          }
        },
        signal: controller.signal,
      }),
    (error: unknown) =>
      error instanceof DOMException && error.name === 'AbortError',
  )
  assert.equal(gateway.installInputs.length, 0)
})

test('rejects desired-retention values outside the user setting range', async () => {
  const gateway = new FakeGateway(activeSnapshot())
  await assert.rejects(
    () => changeDesiredRetention(1, gateway),
    (error: unknown) => error instanceof SchedulingError,
  )
  await assert.rejects(
    () => changeDesiredRetention(0.69, gateway),
    (error: unknown) => error instanceof SchedulingError,
  )
  await assert.rejects(
    () => changeDesiredRetention(0.855, gateway),
    (error: unknown) => error instanceof SchedulingError,
  )
  assert.equal(gateway.preparedRetention, null)
})

function activeSnapshot(): SchedulerReplaySnapshot {
  const config = configRecord(ACTIVE_CONFIG_ID, 0.9)
  return {
    sourceActiveSchedulerConfigId: ACTIVE_CONFIG_ID,
    targetSchedulerConfig: config,
    targetIsNew: false,
    cards: [replayCard(CARD_A, 1_000, config), replayCard(CARD_B, 2_000, config)],
  }
}

function targetSnapshot(
  active: SchedulerReplaySnapshot,
  desiredRetention: number,
): SchedulerReplaySnapshot {
  return {
    sourceActiveSchedulerConfigId: active.sourceActiveSchedulerConfigId,
    targetSchedulerConfig: configRecord(TARGET_CONFIG_ID, desiredRetention),
    targetIsNew: true,
    cards: structuredClone(active.cards),
  }
}

function replayCard(
  reviewCardId: string,
  expectedUpdatedAt: number,
  config: SchedulerConfigRecord,
) {
  return {
    reviewCardId,
    expectedUpdatedAt,
    expectedCardSequence: 1,
    expectedSchedulerConfigId: config.id,
    storedCache: replayReviews(reviewCardId, [review], config).cache,
    storedCacheError: null,
    reviewHistory: [review],
  }
}

function configRecord(
  id: string,
  desiredRetention: number,
): SchedulerConfigRecord {
  const config = structuredClone(DEFAULT_SCHEDULER_CONFIG)
  config.config.desiredRetention = desiredRetention
  return { id, ...config }
}

class FakeGateway implements SchedulerMaintenanceGateway {
  readonly installInputs: InstallSchedulerReplayInput[] = []
  preparedRetention: number | null = null
  private readonly active: SchedulerReplaySnapshot
  private readonly target: SchedulerReplaySnapshot | null

  constructor(
    active: SchedulerReplaySnapshot,
    target: SchedulerReplaySnapshot | null = null,
  ) {
    this.active = active
    this.target = target
  }

  async loadSchedulerReplaySnapshot(): Promise<SchedulerReplaySnapshot> {
    return structuredClone(this.active)
  }

  async prepareDesiredRetentionReplay(
    desiredRetention: number,
  ): Promise<SchedulerReplaySnapshot> {
    this.preparedRetention = desiredRetention
    if (this.target === null) {
      throw new Error('no desired-retention snapshot configured')
    }
    return structuredClone(this.target)
  }

  async installSchedulerReplay(
    input: InstallSchedulerReplayInput,
  ): Promise<SchedulerReplayInstallReport> {
    this.installInputs.push(structuredClone(input))
    return {
      operation: input.operation,
      activeSchedulerConfigId: input.targetSchedulerConfig.id,
      evaluatedCards: input.cards.length,
      installedCards: input.cards.filter((card) => card.install).length,
    }
  }
}
