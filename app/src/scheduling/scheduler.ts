import {
  FSRSVersion,
  Rating,
  State,
  StrategyMode,
  fsrs,
  generatorParameters,
} from 'ts-fsrs'
import type {
  AbstractScheduler,
  Card,
  Grade,
  RecordLogItem,
  TSeedStrategy,
} from 'ts-fsrs'
import {
  TS_FSRS_RUNTIME_VERSION,
  parseSchedulerConfig,
} from './config.ts'
import type {
  GradePreview,
  PreviousReview,
  ReplayResult,
  ReviewCardCache,
  ReviewFact,
  ReviewGrade,
  ScheduleResult,
  ScheduleReviewInput,
  SchedulerConfigV1,
  SchedulerLogV1,
  SchedulerStateV1,
  StudyMoment,
} from './types.ts'
import { ReviewCardState, SchedulingError } from './types.ts'

const MILLISECONDS_PER_DAY = 86_400_000
const SYNTHETIC_TIME_OF_DAY = 12 * 60 * 60 * 1_000

interface SeededCard extends Card {
  daraCardId: string
}

interface SchedulerInternals {
  current: SeededCard
}

export function createNewReviewCardCache(): ReviewCardCache {
  return {
    state: ReviewCardState.New,
    dueAt: null,
    dueStudyDay: null,
    lastReviewAt: null,
    reps: 0,
    lapses: 0,
    schedulerState: null,
  }
}

/** Applies one factual grade to the supplied materialized cache. */
export function scheduleReview(input: ScheduleReviewInput): ScheduleResult {
  const config = parseSchedulerConfig(input.config)
  return scheduleReviewWithValidatedConfig({ ...input, config })
}

/** Computes every grade outcome without changing the supplied cache. */
export function previewReview(
  input: Omit<ScheduleReviewInput, 'review'> & { moment: StudyMoment },
): GradePreview {
  const config = parseSchedulerConfig(input.config)
  const schedule = (grade: ReviewGrade) =>
    scheduleReviewWithValidatedConfig({
      cardId: input.cardId,
      cache: input.cache,
      previousReview: input.previousReview,
      review: { ...input.moment, grade },
      config,
    })

  return {
    1: schedule(1),
    2: schedule(2),
    3: schedule(3),
    4: schedule(4),
  }
}

/**
 * Replays ordered, non-revoked review facts. The persistence layer owns
 * card-sequence ordering and revoke resolution; this kernel owns scheduling.
 */
export function replayReviews(
  cardId: string,
  reviews: readonly ReviewFact[],
  configInput: SchedulerConfigV1,
): ReplayResult {
  const config = parseSchedulerConfig(configInput)
  let cache = createNewReviewCardCache()
  let previousReview: PreviousReview | null = null
  const transitions: ScheduleResult[] = []

  for (const review of reviews) {
    const transition = scheduleReviewWithValidatedConfig({
      cardId,
      cache,
      previousReview,
      review,
      config,
    })
    transitions.push(transition)
    cache = transition.cache
    previousReview = {
      reviewedAt: review.reviewedAt,
      studyDay: review.studyDay,
    }
  }

  return { cache, transitions }
}

export function fuzzSeed(cardId: string, reps: number): string {
  if (cardId.length === 0) {
    throw new SchedulingError('cardId must not be empty')
  }
  requireNonNegativeInteger(reps, 'reps')
  // Alea, used internally by ts-fsrs, hashes this complete string. Keeping the
  // components delimited avoids the arithmetic collisions in its stock helper.
  return `dara-fuzz-v1:${cardId}:${reps}`
}

function scheduleReviewWithValidatedConfig(
  input: ScheduleReviewInput,
): ScheduleResult {
  assertRuntimeVersion()
  assertCardId(input.cardId)
  assertCache(input.cache)
  assertReviewFact(input.review)
  const elapsedDays = elapsedStudyDays(
    input.cache,
    input.previousReview,
    input.review,
  )
  // ts-fsrs calculates elapsed days from Date differences. Day-aligned
  // synthetic instants make that calculation equal Dara's frozen study-day
  // delta; fromFsrsResult maps intraday deadlines back to the real UTC instant.
  const syntheticNow = syntheticReviewTime(input.review.studyDay)
  const card = toFsrsCard(
    input.cardId,
    input.cache,
    input.review,
    elapsedDays,
    syntheticNow,
  )
  const scheduler = schedulerFor(input.config)
  const result = scheduler.next(
    card,
    syntheticNow,
    input.review.grade as Grade,
  )

  return {
    cache: fromFsrsResult(result, input.review, syntheticNow),
    elapsedDays,
    schedulerLog: schedulerLog(input.cache),
  }
}

function schedulerFor(config: SchedulerConfigV1) {
  const parameters = generatorParameters({
    request_retention: config.config.desiredRetention,
    maximum_interval: config.config.maximumInterval,
    w: config.config.parameters,
    enable_fuzz: config.config.fuzzEnabled,
    enable_short_term: true,
    learning_steps: config.config.learningSteps,
    relearning_steps: config.config.relearningSteps,
  })
  return fsrs(parameters).useStrategy(StrategyMode.SEED, daraSeedStrategy)
}

const daraSeedStrategy: TSeedStrategy = function (
  this: AbstractScheduler,
): string {
  const current = Reflect.get(this, 'current') as SchedulerInternals['current']
  return fuzzSeed(current.daraCardId, current.reps)
}

function toFsrsCard(
  cardId: string,
  cache: ReviewCardCache,
  review: ReviewFact,
  elapsedDays: number,
  syntheticNow: Date,
): SeededCard {
  const schedulerState = cache.schedulerState
  const state = toFsrsState(cache.state)
  const due = syntheticDue(cache, review, syntheticNow)
  const lastReview =
    cache.state === ReviewCardState.New
      ? undefined
      : new Date(syntheticNow.getTime() - elapsedDays * MILLISECONDS_PER_DAY)

  return {
    daraCardId: cardId,
    due,
    stability: schedulerState?.stability ?? 0,
    difficulty: schedulerState?.difficulty ?? 0,
    elapsed_days: elapsedDays,
    scheduled_days: schedulerState?.scheduledDays ?? 0,
    learning_steps: schedulerState?.learningSteps ?? 0,
    reps: cache.reps,
    lapses: cache.lapses,
    state,
    last_review: lastReview,
  }
}

function syntheticDue(
  cache: ReviewCardCache,
  review: ReviewFact,
  syntheticNow: Date,
): Date {
  switch (cache.state) {
    case ReviewCardState.New:
      return syntheticNow
    case ReviewCardState.Learning:
    case ReviewCardState.Relearning:
      return new Date(
        syntheticNow.getTime() + (requireValue(cache.dueAt, 'dueAt') - review.reviewedAt),
      )
    case ReviewCardState.Review:
      return new Date(
        syntheticNow.getTime() +
          (requireValue(cache.dueStudyDay, 'dueStudyDay') - review.studyDay) *
            MILLISECONDS_PER_DAY,
      )
  }
}

function fromFsrsResult(
  result: RecordLogItem,
  review: ReviewFact,
  syntheticNow: Date,
): ReviewCardCache {
  const state = fromFsrsState(result.card.state)
  if (state === ReviewCardState.New) {
    throw new SchedulingError('ts-fsrs returned NEW after a grade')
  }

  const schedulerState: SchedulerStateV1 = {
    stability: finite(result.card.stability, 'stability'),
    difficulty: finite(result.card.difficulty, 'difficulty'),
    scheduledDays: nonNegativeInteger(
      result.card.scheduled_days,
      'scheduledDays',
    ),
    learningSteps: nonNegativeInteger(
      result.card.learning_steps,
      'learningSteps',
    ),
  }

  let dueAt: number | null = null
  let dueStudyDay: number | null = null
  if (
    state === ReviewCardState.Learning ||
    state === ReviewCardState.Relearning
  ) {
    const delay = result.card.due.getTime() - syntheticNow.getTime()
    if (!Number.isSafeInteger(delay) || delay <= 0) {
      throw new SchedulingError('ts-fsrs returned an invalid learning delay')
    }
    const exactDueAt = review.reviewedAt + delay
    requireTimestamp(exactDueAt, 'dueAt')
    dueAt = exactDueAt
  } else {
    const dayDue = review.studyDay + schedulerState.scheduledDays
    requireSafeInteger(dayDue, 'dueStudyDay')
    dueStudyDay = dayDue
  }

  return {
    state,
    dueAt,
    dueStudyDay,
    lastReviewAt: review.reviewedAt,
    reps: nonNegativeInteger(result.card.reps, 'reps'),
    lapses: nonNegativeInteger(result.card.lapses, 'lapses'),
    schedulerState,
  }
}

function schedulerLog(cache: ReviewCardCache): SchedulerLogV1 {
  return {
    stateBefore: cache.state,
    dueAtBefore: cache.dueAt,
    dueStudyDayBefore: cache.dueStudyDay,
    stabilityBefore: cache.schedulerState?.stability ?? null,
    difficultyBefore: cache.schedulerState?.difficulty ?? null,
    scheduledDaysBefore: cache.schedulerState?.scheduledDays ?? null,
    learningStepsBefore: cache.schedulerState?.learningSteps ?? null,
  }
}

function elapsedStudyDays(
  cache: ReviewCardCache,
  previousReview: PreviousReview | null,
  review: ReviewFact,
): number {
  if (cache.state === ReviewCardState.New) {
    if (previousReview !== null) {
      throw new SchedulingError('a NEW card cannot have a previous review')
    }
    return 0
  }
  if (previousReview === null) {
    throw new SchedulingError('a non-NEW card requires its previous review')
  }
  assertPreviousReview(previousReview)
  if (cache.lastReviewAt !== previousReview.reviewedAt) {
    throw new SchedulingError(
      'previous review does not match the materialized card cache',
    )
  }
  return Math.max(0, review.studyDay - previousReview.studyDay)
}

function syntheticReviewTime(studyDay: number): Date {
  const milliseconds = studyDay * MILLISECONDS_PER_DAY + SYNTHETIC_TIME_OF_DAY
  if (!Number.isSafeInteger(milliseconds)) {
    throw new SchedulingError('studyDay is outside the supported Date range')
  }
  const date = new Date(milliseconds)
  if (!Number.isFinite(date.getTime())) {
    throw new SchedulingError('studyDay is outside the supported Date range')
  }
  return date
}

function assertCache(cache: ReviewCardCache): void {
  requireNonNegativeInteger(cache.reps, 'cache.reps')
  requireNonNegativeInteger(cache.lapses, 'cache.lapses')
  if (cache.lapses > cache.reps) {
    throw new SchedulingError('cache.lapses cannot exceed cache.reps')
  }

  if (cache.state === ReviewCardState.New) {
    if (
      cache.dueAt !== null ||
      cache.dueStudyDay !== null ||
      cache.lastReviewAt !== null ||
      cache.reps !== 0 ||
      cache.lapses !== 0 ||
      cache.schedulerState !== null
    ) {
      throw new SchedulingError('NEW card cache invariants are violated')
    }
    return
  }

  requireTimestamp(cache.lastReviewAt, 'cache.lastReviewAt')
  if (cache.reps < 1 || cache.schedulerState === null) {
    throw new SchedulingError('non-NEW card cache invariants are violated')
  }
  assertSchedulerState(cache.schedulerState)

  if (cache.state === ReviewCardState.Review) {
    if (cache.dueAt !== null || cache.dueStudyDay === null) {
      throw new SchedulingError('REVIEW due-value invariants are violated')
    }
    requireSafeInteger(cache.dueStudyDay, 'cache.dueStudyDay')
  } else {
    if (cache.dueAt === null || cache.dueStudyDay !== null) {
      throw new SchedulingError('intraday due-value invariants are violated')
    }
    requireTimestamp(cache.dueAt, 'cache.dueAt')
  }
}

function assertSchedulerState(state: SchedulerStateV1): void {
  const stability = finite(state.stability, 'schedulerState.stability')
  const difficulty = finite(state.difficulty, 'schedulerState.difficulty')
  if (stability <= 0) {
    throw new SchedulingError('schedulerState.stability must be positive')
  }
  if (difficulty < 1 || difficulty > 10) {
    throw new SchedulingError('schedulerState.difficulty must be in [1, 10]')
  }
  requireNonNegativeInteger(state.scheduledDays, 'schedulerState.scheduledDays')
  requireNonNegativeInteger(state.learningSteps, 'schedulerState.learningSteps')
}

function assertReviewFact(review: ReviewFact): void {
  if (![1, 2, 3, 4].includes(review.grade)) {
    throw new SchedulingError('grade must be Again, Hard, Good, or Easy')
  }
  requireTimestamp(review.reviewedAt, 'review.reviewedAt')
  requireSafeInteger(review.studyDay, 'review.studyDay')
  requireSafeInteger(review.utcOffsetMinutes, 'review.utcOffsetMinutes')
  if (Math.abs(review.utcOffsetMinutes) > 24 * 60) {
    throw new SchedulingError('review.utcOffsetMinutes is outside valid bounds')
  }
  if (review.timezoneId.trim().length === 0) {
    throw new SchedulingError('review.timezoneId must not be empty')
  }
}

function assertPreviousReview(review: PreviousReview): void {
  requireTimestamp(review.reviewedAt, 'previousReview.reviewedAt')
  requireSafeInteger(review.studyDay, 'previousReview.studyDay')
}

function assertCardId(cardId: string): void {
  if (cardId.trim().length === 0) {
    throw new SchedulingError('cardId must not be empty')
  }
}

function assertRuntimeVersion(): void {
  if (FSRSVersion !== TS_FSRS_RUNTIME_VERSION) {
    throw new SchedulingError(
      `unsupported ts-fsrs runtime ${FSRSVersion}; expected ${TS_FSRS_RUNTIME_VERSION}`,
    )
  }
}

function toFsrsState(state: ReviewCardState): State {
  switch (state) {
    case ReviewCardState.New:
      return State.New
    case ReviewCardState.Learning:
      return State.Learning
    case ReviewCardState.Review:
      return State.Review
    case ReviewCardState.Relearning:
      return State.Relearning
  }
}

function fromFsrsState(state: State): ReviewCardState {
  switch (state) {
    case State.New:
      return ReviewCardState.New
    case State.Learning:
      return ReviewCardState.Learning
    case State.Review:
      return ReviewCardState.Review
    case State.Relearning:
      return ReviewCardState.Relearning
    default:
      throw new SchedulingError(`unsupported ts-fsrs state ${String(state)}`)
  }
}

function requireValue<T>(value: T | null, name: string): T {
  if (value === null) {
    throw new SchedulingError(`${name} must not be null`)
  }
  return value
}

function finite(value: number, name: string): number {
  if (!Number.isFinite(value)) {
    throw new SchedulingError(`${name} must be finite`)
  }
  return value
}

function nonNegativeInteger(value: number, name: string): number {
  requireNonNegativeInteger(value, name)
  return value
}

function requireNonNegativeInteger(value: number, name: string): void {
  requireSafeInteger(value, name)
  if (value < 0) {
    throw new SchedulingError(`${name} must not be negative`)
  }
}

function requireSafeInteger(value: number, name: string): void {
  if (!Number.isSafeInteger(value)) {
    throw new SchedulingError(`${name} must be a safe integer`)
  }
}

function requireTimestamp(value: number | null, name: string): void {
  if (value === null || !Number.isSafeInteger(value) || value < 0) {
    throw new SchedulingError(`${name} must be a UTC millisecond instant`)
  }
}

// Keep Rating in the runtime bundle intentionally: this assertion catches a
// package that changes the persisted 1-4 grade mapping without a config bump.
if (
  Rating.Again !== 1 ||
  Rating.Hard !== 2 ||
  Rating.Good !== 3 ||
  Rating.Easy !== 4
) {
  throw new SchedulingError('ts-fsrs grade values no longer match Dara')
}
