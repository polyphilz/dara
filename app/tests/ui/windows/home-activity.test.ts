import { expect, test } from 'vitest'
import {
  activityLevel,
  buildActivityCalendarData,
  studyDayToIsoDate,
} from '../../../src/windows/main/home-activity.ts'

test('scales review counts and preserves the requested calendar boundaries', () => {
  expect(
    buildActivityCalendarData(
      [
        { studyDay: 20_001, count: 1 },
        { studyDay: 20_003, count: 8 },
      ],
      20_000,
      20_004,
    ),
  ).toEqual([
    { date: studyDayToIsoDate(20_000), count: 0, level: 0 },
    { date: studyDayToIsoDate(20_001), count: 1, level: 1 },
    { date: studyDayToIsoDate(20_003), count: 8, level: 4 },
    { date: studyDayToIsoDate(20_004), count: 0, level: 0 },
  ])
})

test('keeps activity levels within the calendar scale', () => {
  expect(activityLevel(0, 10)).toBe(0)
  expect(activityLevel(2, 10)).toBe(1)
  expect(activityLevel(5, 10)).toBe(2)
  expect(activityLevel(10, 10)).toBe(4)
})
