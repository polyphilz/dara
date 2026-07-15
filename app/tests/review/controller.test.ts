import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { ReviewController } from '../../src/review/controller.ts'
import type {
  RecordGradeInput,
  ReviewContext,
  ReviewGateway,
  ReviewMutationResult,
  ReviewQueueSelection,
  SelectNextReviewCardInput,
  UndoLastGradeInput,
} from '../../src/review/contracts.ts'
import {
  DEFAULT_SCHEDULER_CONFIG,
  createNewReviewCardCache,
} from '../../src/scheduling/index.ts'
import type {
  ReviewCardCache,
  ReviewFact,
  SchedulerLogV1,
  StudyMoment,
} from '../../src/scheduling/index.ts'

interface FixtureStep {
  eventId: string
  review: ReviewFact
  expectedCache: ReviewCardCache
  expectedSchedulerLog: SchedulerLogV1
}

interface PersistenceFixture {
  cardId: string
  schedulerConfigId: string
  steps: FixtureStep[]
}

const fixture = JSON.parse(
  readFileSync(
    'tests/fixtures/scheduling/review-sequence-v1.json',
    'utf8',
  ),
) as PersistenceFixture
const firstStep = fixture.steps[0]!
const moment: StudyMoment = {
  reviewedAt: firstStep.review.reviewedAt,
  studyDay: firstStep.review.studyDay,
  timezoneId: firstStep.review.timezoneId,
  utcOffsetMinutes: firstStep.review.utcOffsetMinutes,
}

type RecordHandler = (
  input: RecordGradeInput,
  call: number,
) => Promise<ReviewMutationResult>
type UndoHandler = (
  input: UndoLastGradeInput,
) => Promise<ReviewMutationResult>

class FakeGateway implements ReviewGateway {
  readonly selectionInputs: SelectNextReviewCardInput[] = []
  readonly recordInputs: RecordGradeInput[] = []
  readonly undoInputs: UndoLastGradeInput[] = []
  private readonly selections: ReviewQueueSelection[]
  private readonly recordHandler: RecordHandler
  private readonly undoHandler: UndoHandler
  private recordCalls = 0

  constructor(
    selections: ReviewQueueSelection[],
    recordHandler: RecordHandler,
    undoHandler: UndoHandler,
  ) {
    this.selections = selections
    this.recordHandler = recordHandler
    this.undoHandler = undoHandler
  }

  async selectNextReviewCard(
    input: SelectNextReviewCardInput,
  ): Promise<ReviewQueueSelection> {
    this.selectionInputs.push(input)
    const selection = this.selections.shift()
    if (!selection) {
      throw new Error('fake selection queue is empty')
    }
    return selection
  }

  async recordGrade(input: RecordGradeInput): Promise<ReviewMutationResult> {
    this.recordInputs.push(input)
    this.recordCalls += 1
    return this.recordHandler(input, this.recordCalls)
  }

  async undoLastGrade(input: UndoLastGradeInput): Promise<ReviewMutationResult> {
    this.undoInputs.push(input)
    return this.undoHandler(input)
  }
}

test('drives queue, preview, grade, caught-up, and undo as one loop', async () => {
  let gateway: FakeGateway
  const initial = initialContext()
  gateway = new FakeGateway(
    [cardSelection(initial), caughtUpSelection(1)],
    async (input) => mutation(input.eventId, 1, gradedContext(input)),
    async (input) => mutation(input.eventId, 2, undoneContext(input.eventId)),
  )
  const ids = [
    '01980c8e-6c00-7000-8000-000000000301',
    '01980c8e-6c00-7000-8000-000000000302',
  ]
  const controller = new ReviewController(gateway, {
    captureMoment: () => moment,
    createEventId: () => ids.shift()!,
  })

  await controller.start()
  assert.equal(controller.getSnapshot().phase, 'QUESTION')
  controller.reveal()
  const revealed = controller.getSnapshot()
  assert.equal(revealed.phase, 'REVEALED')
  if (revealed.phase !== 'REVEALED') {
    assert.fail('expected revealed state')
  }
  assert.equal(revealed.focusedGrade, 3)
  assert.deepEqual(revealed.previews[1].cache, firstStep.expectedCache)

  await controller.submitGrade(1)
  const caughtUp = controller.getSnapshot()
  assert.equal(caughtUp.phase, 'CAUGHT_UP')
  assert.equal(caughtUp.canUndo, true)
  assert.equal(gateway.recordInputs.length, 1)
  assert.deepEqual(gateway.recordInputs[0]!.nextCache, firstStep.expectedCache)
  assert.deepEqual(
    gateway.recordInputs[0]!.schedulerLog,
    firstStep.expectedSchedulerLog,
  )

  await controller.undo()
  const undone = controller.getSnapshot()
  assert.equal(undone.phase, 'QUESTION')
  if (undone.phase !== 'QUESTION') {
    assert.fail('expected restored question state')
  }
  assert.equal(undone.card.context.cache.state, 'NEW')
  assert.equal(undone.notice, 'Last grade undone.')
  assert.equal(undone.canUndo, false)
  assert.equal(gateway.undoInputs[0]!.targetEventId, gateway.recordInputs[0]!.eventId)
  assert.deepEqual(gateway.undoInputs[0]!.nextCache, createNewReviewCardCache())
})

test('retries an uncertain grade with the same event id', async () => {
  const initial = initialContext()
  const gateway = new FakeGateway(
    [cardSelection(initial), caughtUpSelection(1)],
    async (input, call) => {
      if (call === 1) {
        throw { code: 'databaseUnavailable', message: 'writer unavailable' }
      }
      return mutation(input.eventId, 1, gradedContext(input))
    },
    async (input) => mutation(input.eventId, 2, undoneContext(input.eventId)),
  )
  let idCalls = 0
  const eventId = '01980c8e-6c00-7000-8000-000000000303'
  const controller = new ReviewController(gateway, {
    captureMoment: () => moment,
    createEventId: () => {
      idCalls += 1
      return eventId
    },
  })

  await controller.start()
  controller.reveal()
  await controller.submitGrade(1)
  const failed = controller.getSnapshot()
  assert.equal(failed.phase, 'ERROR')
  if (failed.phase !== 'ERROR') {
    assert.fail('expected retryable error')
  }
  assert.equal(failed.canRetry, true)

  await controller.retry()
  assert.equal(controller.getSnapshot().phase, 'CAUGHT_UP')
  assert.equal(gateway.recordInputs.length, 2)
  assert.equal(gateway.recordInputs[0]!.eventId, eventId)
  assert.equal(gateway.recordInputs[1]!.eventId, eventId)
  assert.equal(idCalls, 1)
})

test('reselects stale data without silently resubmitting the grade', async () => {
  const initial = initialContext()
  const refreshed = {
    ...initial,
    reviewCard: { ...initial.reviewCard, updatedAt: 2_000 },
  }
  const gateway = new FakeGateway(
    [cardSelection(initial), cardSelection(refreshed)],
    async () => {
      throw { code: 'staleReviewContext', message: 'stale context' }
    },
    async (input) => mutation(input.eventId, 2, undoneContext(input.eventId)),
  )
  const controller = new ReviewController(gateway, {
    captureMoment: () => moment,
    createEventId: () =>
      '01980c8e-6c00-7000-8000-000000000304',
  })

  await controller.start()
  controller.reveal()
  await controller.submitGrade(1)

  const state = controller.getSnapshot()
  assert.equal(state.phase, 'QUESTION')
  if (state.phase !== 'QUESTION') {
    assert.fail('expected refreshed question')
  }
  assert.equal(state.card.context.reviewCard.updatedAt, 2_000)
  assert.match(state.notice ?? '', /changed before the grade was saved/)
  assert.equal(gateway.recordInputs.length, 1)
  assert.deepEqual(
    gateway.selectionInputs.map((input) => input.normalLaneCursor),
    [0, 0],
  )
})

test('rechecks a caught-up queue when the clock advances after a timer or wake', async () => {
  const context = initialContext()
  const gateway = new FakeGateway(
    [caughtUpSelection(0, moment.reviewedAt + 600_000), cardSelection(context)],
    async (input) => mutation(input.eventId, 1, gradedContext(input)),
    async (input) => mutation(input.eventId, 2, undoneContext(input.eventId)),
  )
  const controller = new ReviewController(gateway, {
    captureMoment: () => moment,
  })

  await controller.start()
  assert.equal(controller.getSnapshot().phase, 'CAUGHT_UP')
  await controller.notifyClockChanged()
  assert.equal(controller.getSnapshot().phase, 'QUESTION')
  assert.equal(gateway.selectionInputs.length, 2)
})

function initialContext(): ReviewContext {
  return {
    cardContent: {
      id: '01980c8e-6c00-7000-8000-000000000101',
      createdAt: 900,
      updatedAt: 1_000,
      type: 'BASIC',
      frontMd: 'fixture front',
      backMd: 'fixture back',
      source: null,
    },
    reviewCard: {
      id: fixture.cardId,
      status: 'ACTIVE',
      variantKey: 'basic',
      updatedAt: 1_000,
    },
    cache: createNewReviewCardCache(),
    cacheSchedulerConfigId: null,
    lastCardSequence: 0,
    schedulerConfig: {
      id: fixture.schedulerConfigId,
      ...DEFAULT_SCHEDULER_CONFIG,
    },
    reviewHistory: [],
  }
}

function gradedContext(input: RecordGradeInput): ReviewContext {
  const initial = initialContext()
  return {
    ...initial,
    reviewCard: { ...initial.reviewCard, updatedAt: 1_001 },
    cache: input.nextCache,
    cacheSchedulerConfigId: fixture.schedulerConfigId,
    lastCardSequence: 1,
    reviewHistory: [
      {
        eventId: input.eventId,
        cardSequence: 1,
        schedulerConfigId: fixture.schedulerConfigId,
        review: input.review,
        schedulerLog: input.schedulerLog,
      },
    ],
  }
}

function undoneContext(_undoEventId: string): ReviewContext {
  const initial = initialContext()
  return {
    ...initial,
    reviewCard: { ...initial.reviewCard, updatedAt: 1_002 },
    lastCardSequence: 2,
  }
}

function cardSelection(context: ReviewContext): ReviewQueueSelection {
  return {
    kind: 'CARD',
    lane: 'NEW',
    nextNormalLaneCursor: 1,
    context,
  }
}

function caughtUpSelection(
  cursor: number,
  nextDueAt: number | null = null,
): ReviewQueueSelection {
  return {
    kind: 'CAUGHT_UP',
    nextDueAt,
    nextNormalLaneCursor: cursor,
  }
}

function mutation(
  eventId: string,
  cardSequence: number,
  context: ReviewContext,
): ReviewMutationResult {
  return {
    disposition: 'APPLIED',
    eventId,
    cardSequence,
    context,
  }
}
