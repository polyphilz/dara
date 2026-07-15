export type ReviewGrade = 1 | 2 | 3 | 4

export const ReviewCardState = {
  New: 'NEW',
  Learning: 'LEARNING',
  Review: 'REVIEW',
  Relearning: 'RELEARNING',
} as const

export type ReviewCardState =
  (typeof ReviewCardState)[keyof typeof ReviewCardState]

export const SchedulerAlgorithm = {
  Fsrs: 'FSRS',
} as const

export type SchedulerAlgorithm =
  (typeof SchedulerAlgorithm)[keyof typeof SchedulerAlgorithm]

export const SchedulerLibrary = {
  TsFsrs: 'ts-fsrs',
} as const

export type SchedulerLibrary =
  (typeof SchedulerLibrary)[keyof typeof SchedulerLibrary]

export type SchedulerStep = `${number}${'m' | 'h' | 'd'}`

export interface StudyMoment {
  reviewedAt: number
  studyDay: number
  timezoneId: string
  utcOffsetMinutes: number
}

export interface ReviewFact extends StudyMoment {
  grade: ReviewGrade
}

export interface SchedulerStateV1 {
  stability: number
  difficulty: number
  scheduledDays: number
  learningSteps: number
}

export interface ReviewCardCache {
  state: ReviewCardState
  dueAt: number | null
  dueStudyDay: number | null
  lastReviewAt: number | null
  reps: number
  lapses: number
  schedulerState: SchedulerStateV1 | null
}

export interface SchedulerConfigJsonV1 {
  parameters: readonly number[]
  desiredRetention: number
  maximumInterval: number
  learningSteps: readonly SchedulerStep[]
  relearningSteps: readonly SchedulerStep[]
  fuzzEnabled: boolean
  fuzzStrategyVersion: 1
}

export interface SchedulerConfigV1 {
  algorithm: SchedulerAlgorithm
  algorithmVersion: 6
  schedulerLibrary: SchedulerLibrary
  libraryVersion: '5.4.1'
  configSchemaVersion: 1
  config: SchedulerConfigJsonV1
}

export interface SchedulerLogV1 {
  stateBefore: ReviewCardState
  dueAtBefore: number | null
  dueStudyDayBefore: number | null
  stabilityBefore: number | null
  difficultyBefore: number | null
  scheduledDaysBefore: number | null
  learningStepsBefore: number | null
}

export interface PreviousReview {
  reviewedAt: number
  studyDay: number
}

export interface ScheduleReviewInput {
  cardId: string
  cache: ReviewCardCache
  previousReview: PreviousReview | null
  review: ReviewFact
  config: SchedulerConfigV1
}

export interface ScheduleResult {
  cache: ReviewCardCache
  elapsedDays: number
  schedulerLog: SchedulerLogV1
}

export interface ReplayResult {
  cache: ReviewCardCache
  transitions: readonly ScheduleResult[]
}

export type GradePreview = Readonly<Record<ReviewGrade, ScheduleResult>>

export class SchedulingError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'SchedulingError'
  }
}
