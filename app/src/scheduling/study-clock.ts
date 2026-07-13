import type { StudyMoment } from './types.ts'
import { SchedulingError } from './types.ts'

const MILLISECONDS_PER_MINUTE = 60_000
const MILLISECONDS_PER_DAY = 86_400_000
const DAY_BOUNDARY_HOUR = 4

interface CivilTime {
  year: number
  month: number
  day: number
  hour: number
  minute: number
  second: number
}

const formatterCache = new Map<string, Intl.DateTimeFormat>()

export function captureStudyMoment(
  instant: number | Date = Date.now(),
  timezoneId: string = currentTimezoneId(),
): StudyMoment {
  const reviewedAt = epochMilliseconds(instant)
  const formatter = formatterFor(timezoneId)
  const canonicalTimezoneId = formatter.resolvedOptions().timeZone
  const civil = civilTime(formatter, reviewedAt)

  let studyDay = civilDayOrdinal(civil.year, civil.month, civil.day)
  if (civil.hour < DAY_BOUNDARY_HOUR) {
    studyDay -= 1
  }

  const instantAtWholeSecond = Math.floor(reviewedAt / 1_000) * 1_000
  const civilAsUtc = utcMilliseconds(
    civil.year,
    civil.month,
    civil.day,
    civil.hour,
    civil.minute,
    civil.second,
  )
  const utcOffsetMinutes =
    (civilAsUtc - instantAtWholeSecond) / MILLISECONDS_PER_MINUTE

  if (!Number.isSafeInteger(utcOffsetMinutes)) {
    throw new SchedulingError(
      `timezone ${canonicalTimezoneId} produced a non-integral minute offset`,
    )
  }

  return {
    reviewedAt,
    studyDay,
    timezoneId: canonicalTimezoneId,
    utcOffsetMinutes,
  }
}

export function currentTimezoneId(): string {
  const timezoneId = Intl.DateTimeFormat().resolvedOptions().timeZone
  if (!timezoneId) {
    throw new SchedulingError('the device did not report an IANA timezone')
  }
  return timezoneId
}

export function civilDayOrdinal(
  year: number,
  month: number,
  day: number,
): number {
  if (![year, month, day].every(Number.isSafeInteger)) {
    throw new SchedulingError('civil date components must be safe integers')
  }
  if (month < 1 || month > 12 || day < 1 || day > 31) {
    throw new SchedulingError('civil date components are outside valid bounds')
  }
  const milliseconds = utcMilliseconds(year, month, day, 0, 0, 0)
  const roundTrip = new Date(milliseconds)
  if (
    roundTrip.getUTCFullYear() !== year ||
    roundTrip.getUTCMonth() !== month - 1 ||
    roundTrip.getUTCDate() !== day
  ) {
    throw new SchedulingError('civil date does not exist')
  }
  return Math.floor(milliseconds / MILLISECONDS_PER_DAY)
}

function formatterFor(timezoneId: string): Intl.DateTimeFormat {
  const existing = formatterCache.get(timezoneId)
  if (existing) {
    return existing
  }

  let formatter: Intl.DateTimeFormat
  try {
    formatter = new Intl.DateTimeFormat('en-US-u-ca-gregory-nu-latn', {
      timeZone: timezoneId,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hourCycle: 'h23',
    })
  } catch (error) {
    const detail = error instanceof Error ? `: ${error.message}` : ''
    throw new SchedulingError(`invalid IANA timezone ${timezoneId}${detail}`)
  }

  formatterCache.set(timezoneId, formatter)
  return formatter
}

function civilTime(formatter: Intl.DateTimeFormat, instant: number): CivilTime {
  const values = new Map<string, string>()
  for (const part of formatter.formatToParts(instant)) {
    if (part.type !== 'literal') {
      values.set(part.type, part.value)
    }
  }

  const result = {
    year: partNumber(values, 'year'),
    month: partNumber(values, 'month'),
    day: partNumber(values, 'day'),
    hour: partNumber(values, 'hour'),
    minute: partNumber(values, 'minute'),
    second: partNumber(values, 'second'),
  }
  if (result.hour === 24) {
    result.hour = 0
  }
  return result
}

function partNumber(values: Map<string, string>, name: string): number {
  const value = values.get(name)
  if (value === undefined) {
    throw new SchedulingError(`Intl omitted the ${name} date part`)
  }
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed)) {
    throw new SchedulingError(`Intl returned an invalid ${name} date part`)
  }
  return parsed
}

function epochMilliseconds(value: number | Date): number {
  const milliseconds = value instanceof Date ? value.getTime() : value
  if (
    !Number.isSafeInteger(milliseconds) ||
    milliseconds < 0 ||
    !Number.isFinite(new Date(milliseconds).getTime())
  ) {
    throw new SchedulingError(
      'reviewedAt must be a non-negative integer UTC millisecond instant',
    )
  }
  return milliseconds
}

function utcMilliseconds(
  year: number,
  month: number,
  day: number,
  hour: number,
  minute: number,
  second: number,
): number {
  const date = new Date(0)
  date.setUTCFullYear(year, month - 1, day)
  date.setUTCHours(hour, minute, second, 0)
  const milliseconds = date.getTime()
  if (!Number.isFinite(milliseconds)) {
    throw new SchedulingError('civil date is outside the supported range')
  }
  return milliseconds
}
