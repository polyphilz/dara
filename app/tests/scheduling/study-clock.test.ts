import assert from 'node:assert/strict'
import test from 'node:test'
import {
  SchedulingError,
  captureStudyMoment,
  civilDayOrdinal,
  nextStudyDayBoundary,
} from '../../src/scheduling/index.ts'

test('uses a 4AM local boundary instead of midnight', () => {
  const beforeMidnight = captureStudyMoment(
    Date.parse('2026-07-13T03:50:00Z'),
    'America/New_York',
  )
  const afterMidnight = captureStudyMoment(
    Date.parse('2026-07-13T04:10:00Z'),
    'America/New_York',
  )
  const beforeBoundary = captureStudyMoment(
    Date.parse('2026-07-13T07:59:59Z'),
    'America/New_York',
  )
  const atBoundary = captureStudyMoment(
    Date.parse('2026-07-13T08:00:00Z'),
    'America/New_York',
  )

  assert.equal(beforeMidnight.studyDay, civilDayOrdinal(2026, 7, 12))
  assert.equal(afterMidnight.studyDay, beforeMidnight.studyDay)
  assert.equal(beforeBoundary.studyDay, civilDayOrdinal(2026, 7, 12))
  assert.equal(atBoundary.studyDay, civilDayOrdinal(2026, 7, 13))
  assert.equal(atBoundary.studyDay - beforeBoundary.studyDay, 1)
  assert.equal(atBoundary.utcOffsetMinutes, -240)
})

test('captures spring DST without inventing a study-day transition', () => {
  const beforeJump = captureStudyMoment(
    Date.parse('2026-03-08T06:55:00Z'),
    'America/New_York',
  )
  const afterJump = captureStudyMoment(
    Date.parse('2026-03-08T07:05:00Z'),
    'America/New_York',
  )
  const afterBoundary = captureStudyMoment(
    Date.parse('2026-03-08T08:05:00Z'),
    'America/New_York',
  )

  assert.equal(beforeJump.utcOffsetMinutes, -300)
  assert.equal(afterJump.utcOffsetMinutes, -240)
  assert.equal(beforeJump.studyDay, civilDayOrdinal(2026, 3, 7))
  assert.equal(afterJump.studyDay, beforeJump.studyDay)
  assert.equal(afterBoundary.studyDay, civilDayOrdinal(2026, 3, 8))
})

test('distinguishes both repeated fall DST instants', () => {
  const firstOneThirty = captureStudyMoment(
    Date.parse('2026-11-01T05:30:00Z'),
    'America/New_York',
  )
  const secondOneThirty = captureStudyMoment(
    Date.parse('2026-11-01T06:30:00Z'),
    'America/New_York',
  )

  assert.equal(firstOneThirty.utcOffsetMinutes, -240)
  assert.equal(secondOneThirty.utcOffsetMinutes, -300)
  assert.equal(firstOneThirty.studyDay, civilDayOrdinal(2026, 10, 31))
  assert.equal(secondOneThirty.studyDay, firstOneThirty.studyDay)
})

test('finds the next 4AM boundary across ordinary and DST-short days', () => {
  assert.equal(
    nextStudyDayBoundary(
      Date.parse('2026-07-13T04:10:00Z'),
      'America/New_York',
    ),
    Date.parse('2026-07-13T08:00:00Z'),
  )
  assert.equal(
    nextStudyDayBoundary(
      Date.parse('2026-03-07T09:00:00Z'),
      'America/New_York',
    ),
    Date.parse('2026-03-08T08:00:00Z'),
  )
})

test('finds the next 4AM boundary across a DST-long day', () => {
  assert.equal(
    nextStudyDayBoundary(
      Date.parse('2026-10-31T08:00:00Z'),
      'America/New_York',
    ),
    Date.parse('2026-11-01T09:00:00Z'),
  )
})

test('freezes the zone and study day observed during travel', () => {
  const instant = Date.parse('2026-07-13T08:30:00Z')
  const newYork = captureStudyMoment(instant, 'America/New_York')
  const losAngeles = captureStudyMoment(instant, 'America/Los_Angeles')
  const tokyo = captureStudyMoment(instant, 'Asia/Tokyo')

  assert.equal(newYork.studyDay, civilDayOrdinal(2026, 7, 13))
  assert.equal(losAngeles.studyDay, civilDayOrdinal(2026, 7, 12))
  assert.equal(tokyo.studyDay, civilDayOrdinal(2026, 7, 13))
  assert.equal(newYork.timezoneId, 'America/New_York')
  assert.equal(losAngeles.utcOffsetMinutes, -420)
  assert.equal(tokyo.utcOffsetMinutes, 540)
})

test('uses the canonical civil-day ordinal', () => {
  assert.equal(civilDayOrdinal(1970, 1, 1), 0)
  assert.equal(civilDayOrdinal(1969, 12, 31), -1)
  assert.equal(civilDayOrdinal(2026, 7, 13) - civilDayOrdinal(2026, 7, 12), 1)
  assert.throws(() => civilDayOrdinal(2026, 2, 30), /does not exist/)
})

test('rejects invalid clock inputs', () => {
  assert.throws(
    () => captureStudyMoment(-1, 'UTC'),
    (error: unknown) => error instanceof SchedulingError,
  )
  assert.throws(
    () => captureStudyMoment(Date.now(), 'Not/A_Zone'),
    /invalid IANA timezone/,
  )
})
