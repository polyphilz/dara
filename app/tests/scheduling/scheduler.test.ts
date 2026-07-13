import assert from 'node:assert/strict'
import test from 'node:test'
import {
  DEFAULT_SCHEDULER_CONFIG,
  SchedulingError,
  captureStudyMoment,
  createNewReviewCardCache,
  fuzzSeed,
  parseSchedulerConfig,
  previewReview,
  replayReviews,
  scheduleReview,
} from '../../src/scheduling/index.ts'
import type {
  PreviousReview,
  ReviewCardCache,
  ReviewFact,
  ReviewGrade,
  SchedulerConfigV1,
} from '../../src/scheduling/index.ts'

const CARD_ID = '01980c8e-6c00-7000-8000-000000000102'
const MILLISECONDS_PER_DAY = 86_400_000

test('parses the exact seeded FSRS-6 configuration and rejects drift', () => {
  const serialized = JSON.parse(
    JSON.stringify(DEFAULT_SCHEDULER_CONFIG),
  ) as unknown
  assert.deepEqual(parseSchedulerConfig(serialized), DEFAULT_SCHEDULER_CONFIG)

  const wrongVersion = mutableConfig()
  wrongVersion.libraryVersion = '5.4.2' as '5.4.1'
  assert.throws(
    () => parseSchedulerConfig(wrongVersion),
    /libraryVersion must be "5.4.1"/,
  )

  const wrongParameters = mutableConfig()
  wrongParameters.config.parameters = wrongParameters.config.parameters.slice(0, 19)
  assert.throws(
    () => parseSchedulerConfig(wrongParameters),
    /FSRS-6 requires exactly 21 parameters/,
  )

  const wrongSteps = mutableConfig()
  wrongSteps.config.learningSteps = ['1m', '10m']
  assert.throws(
    () => parseSchedulerConfig(wrongSteps),
    /requires learningSteps \["10m"\]/,
  )
})

test('keeps a ten-minute learning deadline exact across the 4AM boundary', () => {
  const firstReview = fact(1, '2026-07-13T07:55:00Z', 'America/New_York')
  const first = scheduleReview({
    cardId: CARD_ID,
    cache: createNewReviewCardCache(),
    previousReview: null,
    review: firstReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })

  assert.equal(first.elapsedDays, 0)
  assert.equal(first.cache.state, 'LEARNING')
  assert.equal(first.cache.dueAt, firstReview.reviewedAt + 10 * 60_000)
  assert.equal(first.cache.dueStudyDay, null)
  assert.equal(first.cache.reps, 1)
  assert.equal(first.schedulerLog.stateBefore, 'NEW')
  assert.equal(first.schedulerLog.stabilityBefore, null)

  const secondReview = fact(3, '2026-07-13T08:05:00Z', 'America/New_York')
  assert.equal(secondReview.reviewedAt, first.cache.dueAt)
  assert.equal(secondReview.studyDay - firstReview.studyDay, 1)

  const second = scheduleReview({
    cardId: CARD_ID,
    cache: first.cache,
    previousReview: previous(firstReview),
    review: secondReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })

  assert.equal(second.elapsedDays, 1)
  assert.equal(second.cache.state, 'REVIEW')
  assert.equal(second.cache.dueAt, null)
  assert.ok(second.cache.dueStudyDay !== null)
  assert.ok(second.cache.dueStudyDay > secondReview.studyDay)
  assert.equal(second.schedulerLog.stateBefore, 'LEARNING')
  assert.doesNotThrow(() => JSON.stringify(second))

  const replay = replayReviews(
    CARD_ID,
    [firstReview, secondReview],
    DEFAULT_SCHEDULER_CONFIG,
  )
  assert.deepEqual(replay.cache, second.cache)
  assert.deepEqual(replay.transitions, [first, second])
})

test('previews the four grades without mutating the materialized cache', () => {
  const cache = createNewReviewCardCache()
  const original = structuredClone(cache)
  const moment = captureStudyMoment(
    Date.parse('2026-07-13T16:00:00Z'),
    'America/New_York',
  )
  const preview = previewReview({
    cardId: CARD_ID,
    cache,
    previousReview: null,
    moment,
    config: DEFAULT_SCHEDULER_CONFIG,
  })

  assert.equal(preview[1].cache.state, 'LEARNING')
  assert.equal(preview[1].cache.dueAt, moment.reviewedAt + 10 * 60_000)
  assert.equal(preview[2].cache.state, 'LEARNING')
  assert.equal(preview[2].cache.dueAt, moment.reviewedAt + 15 * 60_000)
  assert.equal(preview[3].cache.state, 'REVIEW')
  assert.equal(preview[4].cache.state, 'REVIEW')
  assert.deepEqual(cache, original)
})

test('matches the pinned first-review golden outputs', () => {
  const moment = captureStudyMoment(
    Date.parse('2026-07-13T16:00:00Z'),
    'UTC',
  )
  const preview = previewReview({
    cardId: CARD_ID,
    cache: createNewReviewCardCache(),
    previousReview: null,
    moment,
    config: DEFAULT_SCHEDULER_CONFIG,
  })

  assert.deepEqual(preview[1].cache.schedulerState, {
    stability: 0.212,
    difficulty: 6.4133,
    scheduledDays: 0,
    learningSteps: 0,
  })
  assert.deepEqual(preview[3].cache.schedulerState, {
    stability: 2.3065,
    difficulty: 2.11810397,
    scheduledDays: 2,
    learningSteps: 0,
  })
  assert.deepEqual(preview[4].cache.schedulerState, {
    stability: 8.2956,
    difficulty: 1,
    scheduledDays: 10,
    learningSteps: 0,
  })
})

test('replays a learning step through the spring DST jump', () => {
  const beforeJump = fact(1, '2026-03-08T06:55:00Z', 'America/New_York')
  const first = scheduleReview({
    cardId: CARD_ID,
    cache: createNewReviewCardCache(),
    previousReview: null,
    review: beforeJump,
    config: DEFAULT_SCHEDULER_CONFIG,
  })
  const afterJump = fact(3, '2026-03-08T07:05:00Z', 'America/New_York')

  assert.equal(first.cache.dueAt, afterJump.reviewedAt)
  assert.equal(afterJump.studyDay, beforeJump.studyDay)

  const second = scheduleReview({
    cardId: CARD_ID,
    cache: first.cache,
    previousReview: previous(beforeJump),
    review: afterJump,
    config: DEFAULT_SCHEDULER_CONFIG,
  })
  assert.equal(second.elapsedDays, 0)
  assert.deepEqual(
    replayReviews(
      CARD_ID,
      [beforeJump, afterJump],
      DEFAULT_SCHEDULER_CONFIG,
    ).cache,
    second.cache,
  )
})

test('uses frozen study days rather than UTC elapsed time', () => {
  const firstReview = fact(3, '2026-03-07T17:00:00Z', 'America/New_York')
  const first = scheduleReview({
    cardId: CARD_ID,
    cache: createNewReviewCardCache(),
    previousReview: null,
    review: firstReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })
  assert.equal(first.cache.state, 'REVIEW')

  const nextStudyDay = requireDueStudyDay(first.cache)
  const secondMoment = momentForUtcStudyDay(nextStudyDay)
  const secondReview: ReviewFact = { ...secondMoment, grade: 3 }
  const second = scheduleReview({
    cardId: CARD_ID,
    cache: first.cache,
    previousReview: previous(firstReview),
    review: secondReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })

  assert.equal(second.elapsedDays, nextStudyDay - firstReview.studyDay)

  const westwardClockChange: ReviewFact = {
    ...secondReview,
    reviewedAt: secondReview.reviewedAt + MILLISECONDS_PER_DAY,
    studyDay: firstReview.studyDay - 1,
    timezoneId: 'Pacific/Honolulu',
    utcOffsetMinutes: -600,
  }
  const clamped = scheduleReview({
    cardId: CARD_ID,
    cache: first.cache,
    previousReview: previous(firstReview),
    review: westwardClockChange,
    config: DEFAULT_SCHEDULER_CONFIG,
  })
  assert.equal(clamped.elapsedDays, 0)
})

test('turns a failed review into exact relearning and increments lapses', () => {
  const firstReview = fact(3, '2026-07-13T16:00:00Z', 'UTC')
  const learned = scheduleReview({
    cardId: CARD_ID,
    cache: createNewReviewCardCache(),
    previousReview: null,
    review: firstReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })
  const dueStudyDay = requireDueStudyDay(learned.cache)
  const lapseReview: ReviewFact = {
    ...momentForUtcStudyDay(dueStudyDay),
    grade: 1,
  }
  const lapsed = scheduleReview({
    cardId: CARD_ID,
    cache: learned.cache,
    previousReview: previous(firstReview),
    review: lapseReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })

  assert.equal(lapsed.cache.state, 'RELEARNING')
  assert.equal(lapsed.cache.lapses, 1)
  assert.equal(lapsed.cache.dueStudyDay, null)
  assert.equal(lapsed.cache.dueAt, lapseReview.reviewedAt + 10 * 60_000)
})

test('full replay equals live transitions and undo is replay without the target fact', () => {
  const reviews: ReviewFact[] = []
  let cache = createNewReviewCardCache()
  let previousReview: PreviousReview | null = null
  const cacheAfterEachReview: ReviewCardCache[] = []

  const first = fact(1, '2026-07-13T16:00:00Z', 'UTC')
  reviews.push(first)
  cache = applyLive(cache, previousReview, first)
  previousReview = previous(first)
  cacheAfterEachReview.push(cache)

  const second = fact(3, '2026-07-13T16:10:00Z', 'UTC')
  reviews.push(second)
  cache = applyLive(cache, previousReview, second)
  previousReview = previous(second)
  cacheAfterEachReview.push(cache)

  const thirdDay = requireDueStudyDay(cache)
  const third: ReviewFact = { ...momentForUtcStudyDay(thirdDay), grade: 2 }
  reviews.push(third)
  cache = applyLive(cache, previousReview, third)
  cacheAfterEachReview.push(cache)

  assert.deepEqual(
    replayReviews(CARD_ID, reviews, DEFAULT_SCHEDULER_CONFIG).cache,
    cache,
  )
  assert.deepEqual(
    replayReviews(CARD_ID, reviews.slice(0, -1), DEFAULT_SCHEDULER_CONFIG).cache,
    cacheAfterEachReview[1],
  )
})

test('fuzz is deterministic and seeded by delimited card identity plus reps', () => {
  assert.equal(
    fuzzSeed('card-12', 3),
    'dara-fuzz-v1:card-12:3',
  )
  assert.notEqual(fuzzSeed('card-1', 23), fuzzSeed('card-12', 3))

  const previousStudyDay = 20_000
  const currentStudyDay = previousStudyDay + 100
  const cache = matureReviewCache(previousStudyDay)
  const review: ReviewFact = {
    ...momentForUtcStudyDay(currentStudyDay),
    grade: 3,
  }
  const input = {
    cardId: CARD_ID,
    cache,
    previousReview: {
      reviewedAt: cache.lastReviewAt!,
      studyDay: previousStudyDay,
    },
    review,
    config: DEFAULT_SCHEDULER_CONFIG,
  }

  assert.deepEqual(scheduleReview(input), scheduleReview(input))

  const scheduledDays = new Set<number>()
  for (let index = 0; index < 64; index += 1) {
    const result = scheduleReview({ ...input, cardId: `${CARD_ID}:${index}` })
    scheduledDays.add(result.cache.schedulerState!.scheduledDays)
  }
  assert.ok(scheduledDays.size > 1)
})

test('fuzz remains monotonic as the persisted elapsed day increases', () => {
  const previousStudyDay = 20_000
  const cache = matureReviewCache(previousStudyDay)
  const intervals = [0, 1, 2, 5, 10, 20, 40, 80, 100, 120, 160].map(
    (elapsedDays) => {
      const review: ReviewFact = {
        ...momentForUtcStudyDay(previousStudyDay + elapsedDays),
        grade: 3,
      }
      return scheduleReview({
        cardId: CARD_ID,
        cache,
        previousReview: {
          reviewedAt: cache.lastReviewAt!,
          studyDay: previousStudyDay,
        },
        review,
        config: DEFAULT_SCHEDULER_CONFIG,
      }).cache.schedulerState!.scheduledDays
    },
  )

  for (let index = 1; index < intervals.length; index += 1) {
    assert.ok(intervals[index]! >= intervals[index - 1]!)
  }
})

test('rejects stale previous-review context and invalid cache combinations', () => {
  const firstReview = fact(1, '2026-07-13T16:00:00Z', 'UTC')
  const first = scheduleReview({
    cardId: CARD_ID,
    cache: createNewReviewCardCache(),
    previousReview: null,
    review: firstReview,
    config: DEFAULT_SCHEDULER_CONFIG,
  })
  const nextReview = fact(3, '2026-07-13T16:10:00Z', 'UTC')

  assert.throws(
    () =>
      scheduleReview({
        cardId: CARD_ID,
        cache: first.cache,
        previousReview: null,
        review: nextReview,
        config: DEFAULT_SCHEDULER_CONFIG,
      }),
    /requires its previous review/,
  )
  assert.throws(
    () =>
      scheduleReview({
        cardId: CARD_ID,
        cache: first.cache,
        previousReview: { reviewedAt: 1, studyDay: firstReview.studyDay },
        review: nextReview,
        config: DEFAULT_SCHEDULER_CONFIG,
      }),
    /does not match the materialized card cache/,
  )

  const invalidNew = createNewReviewCardCache()
  invalidNew.dueStudyDay = firstReview.studyDay
  assert.throws(
    () =>
      scheduleReview({
        cardId: CARD_ID,
        cache: invalidNew,
        previousReview: null,
        review: firstReview,
        config: DEFAULT_SCHEDULER_CONFIG,
      }),
    (error: unknown) => error instanceof SchedulingError,
  )
})

function fact(
  grade: ReviewGrade,
  isoInstant: string,
  timezoneId: string,
): ReviewFact {
  return {
    ...captureStudyMoment(Date.parse(isoInstant), timezoneId),
    grade,
  }
}

function previous(review: ReviewFact): PreviousReview {
  return { reviewedAt: review.reviewedAt, studyDay: review.studyDay }
}

function applyLive(
  cache: ReviewCardCache,
  previousReview: PreviousReview | null,
  review: ReviewFact,
): ReviewCardCache {
  return scheduleReview({
    cardId: CARD_ID,
    cache,
    previousReview,
    review,
    config: DEFAULT_SCHEDULER_CONFIG,
  }).cache
}

function momentForUtcStudyDay(studyDay: number) {
  return captureStudyMoment(
    studyDay * MILLISECONDS_PER_DAY + 12 * 60 * 60 * 1_000,
    'UTC',
  )
}

function requireDueStudyDay(cache: ReviewCardCache): number {
  assert.notEqual(cache.dueStudyDay, null)
  return cache.dueStudyDay!
}

function matureReviewCache(previousStudyDay: number): ReviewCardCache {
  return {
    state: 'REVIEW',
    dueAt: null,
    dueStudyDay: previousStudyDay + 100,
    lastReviewAt:
      previousStudyDay * MILLISECONDS_PER_DAY + 12 * 60 * 60 * 1_000,
    reps: 10,
    lapses: 1,
    schedulerState: {
      stability: 100,
      difficulty: 5,
      scheduledDays: 100,
      learningSteps: 0,
    },
  }
}

function mutableConfig(): {
  -readonly [Key in keyof SchedulerConfigV1]: SchedulerConfigV1[Key]
} & {
  config: {
    -readonly [Key in keyof SchedulerConfigV1['config']]: SchedulerConfigV1['config'][Key]
  }
} {
  return JSON.parse(JSON.stringify(DEFAULT_SCHEDULER_CONFIG))
}
