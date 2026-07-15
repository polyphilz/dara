import type { Activity } from 'react-activity-calendar'
import type { DailyReviewActivity } from '../../review/index.ts'

const MILLISECONDS_PER_DAY = 86_400_000
const MAX_ACTIVITY_LEVEL = 4

export function buildActivityCalendarData(
  activity: DailyReviewActivity[],
  startStudyDay: number,
  endStudyDay: number,
): Activity[] {
  const counts = new Map<number, number>([
    [startStudyDay, 0],
    [endStudyDay, 0],
  ])
  for (const day of activity) {
    if (day.studyDay >= startStudyDay && day.studyDay <= endStudyDay) {
      counts.set(day.studyDay, day.count)
    }
  }

  const maxCount = Math.max(...counts.values())
  return [...counts.entries()]
    .sort(([left], [right]) => left - right)
    .map(([studyDay, count]) => ({
      date: studyDayToIsoDate(studyDay),
      count,
      level: activityLevel(count, maxCount),
    }))
}

export function activityLevel(count: number, maxCount: number): number {
  if (count <= 0 || maxCount <= 0) {
    return 0
  }
  return Math.max(
    1,
    Math.min(MAX_ACTIVITY_LEVEL, Math.ceil((count / maxCount) * MAX_ACTIVITY_LEVEL)),
  )
}

export function studyDayToIsoDate(studyDay: number): string {
  return new Date(studyDay * MILLISECONDS_PER_DAY)
    .toISOString()
    .slice(0, 10)
}
