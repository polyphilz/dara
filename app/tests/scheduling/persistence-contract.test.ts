import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  DEFAULT_SCHEDULER_CONFIG,
  createNewReviewCardCache,
  replayReviews,
  scheduleReview,
} from '../../src/scheduling/index.ts'
import type {
  PreviousReview,
  ReviewCardCache,
  ReviewFact,
  SchedulerLogV1,
} from '../../src/scheduling/index.ts'

interface ContractStep {
  eventId: string
  review: ReviewFact
  expectedElapsedDays: number
  expectedCache: ReviewCardCache
  expectedSchedulerLog: SchedulerLogV1
}

interface PersistenceContractV1 {
  schemaVersion: 1
  cardId: string
  schedulerConfigId: string
  steps: ContractStep[]
  undo: {
    eventId: string
    targetEventId: string
    expectedCache: ReviewCardCache
  }
}

const fixture = JSON.parse(
  readFileSync(
    'tests/fixtures/scheduling/review-sequence-v1.json',
    'utf8',
  ),
) as PersistenceContractV1

test('shared persistence fixture matches live scheduling, replay, and undo', () => {
  assert.equal(fixture.schemaVersion, 1)
  assert.match(fixture.schedulerConfigId, /^[0-9a-f-]{36}$/)

  let cache = createNewReviewCardCache()
  let previousReview: PreviousReview | null = null
  const reviews: ReviewFact[] = []
  const eventIds = new Set<string>()

  for (const step of fixture.steps) {
    assert.equal(eventIds.has(step.eventId), false)
    eventIds.add(step.eventId)
    const result = scheduleReview({
      cardId: fixture.cardId,
      cache,
      previousReview,
      review: step.review,
      config: DEFAULT_SCHEDULER_CONFIG,
    })
    assert.equal(result.elapsedDays, step.expectedElapsedDays)
    assert.deepEqual(result.schedulerLog, step.expectedSchedulerLog)
    assert.deepEqual(result.cache, step.expectedCache)

    reviews.push(step.review)
    cache = result.cache
    previousReview = {
      reviewedAt: step.review.reviewedAt,
      studyDay: step.review.studyDay,
    }
  }

  assert.deepEqual(
    replayReviews(fixture.cardId, reviews, DEFAULT_SCHEDULER_CONFIG).cache,
    cache,
  )
  assert.equal(fixture.undo.targetEventId, fixture.steps.at(-1)?.eventId)
  assert.deepEqual(
    replayReviews(
      fixture.cardId,
      reviews.slice(0, -1),
      DEFAULT_SCHEDULER_CONFIG,
    ).cache,
    fixture.undo.expectedCache,
  )
})
