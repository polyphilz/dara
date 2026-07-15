import type {
  GradePreview,
  ReviewCardCache,
  ReviewCardState,
  ReviewFact,
  SchedulerConfigV1,
  SchedulerLogV1,
} from '../scheduling/index.ts'

export const ReviewCardStatus = {
  Active: 'ACTIVE',
  Suspended: 'SUSPENDED',
} as const

export type ReviewCardStatus =
  (typeof ReviewCardStatus)[keyof typeof ReviewCardStatus]

export const ReviewQueueLane = {
  Intraday: 'INTRADAY',
  Review: 'REVIEW',
  New: 'NEW',
} as const

export type ReviewQueueLane =
  (typeof ReviewQueueLane)[keyof typeof ReviewQueueLane]

export const MutationDisposition = {
  Applied: 'APPLIED',
  AlreadyApplied: 'ALREADY_APPLIED',
} as const

export type MutationDisposition =
  (typeof MutationDisposition)[keyof typeof MutationDisposition]

export const CardContentType = {
  Basic: 'BASIC',
} as const

export type CardContentType =
  (typeof CardContentType)[keyof typeof CardContentType]

export interface BasicCardContent {
  id: string
  createdAt: number
  updatedAt: number
  type: typeof CardContentType.Basic
  frontMd: string
  backMd: string
  source: string | null
}

export type CardContent = BasicCardContent

export type CardContentDraft = Pick<
  BasicCardContent,
  'type' | 'frontMd' | 'backMd' | 'source'
>

export const CardContentReviewStatus = {
  Active: 'ACTIVE',
  Suspended: 'SUSPENDED',
  Mixed: 'MIXED',
} as const

export type CardContentReviewStatus =
  (typeof CardContentReviewStatus)[keyof typeof CardContentReviewStatus]

export const ReviewQueueSelectionKind = {
  Card: 'CARD',
  CaughtUp: 'CAUGHT_UP',
} as const

export type ReviewQueueSelectionKind =
  (typeof ReviewQueueSelectionKind)[keyof typeof ReviewQueueSelectionKind]

export interface CardContentListItem {
  cardContent: CardContent
  reviewStatus: CardContentReviewStatus
  lifecycleUpdatedAt: number
}

export interface ReviewCardSummary {
  id: string
  status: ReviewCardStatus
  variantKey: string
  updatedAt: number
}

export interface SchedulerConfigRecord extends SchedulerConfigV1 {
  id: string
}

export interface PersistedReviewFact {
  eventId: string
  cardSequence: number
  schedulerConfigId: string
  review: ReviewFact
  schedulerLog: SchedulerLogV1
}

export interface ReviewContext {
  cardContent: CardContent
  reviewCard: ReviewCardSummary
  cache: ReviewCardCache
  cacheSchedulerConfigId: string | null
  lastCardSequence: number
  schedulerConfig: SchedulerConfigRecord
  reviewHistory: PersistedReviewFact[]
}

export interface SelectNextReviewCardInput {
  now: number
  studyDay: number
  normalLaneCursor: number
}

export type ReviewQueueSelection =
  | {
      kind: typeof ReviewQueueSelectionKind.Card
      lane: ReviewQueueLane
      nextNormalLaneCursor: number
      context: ReviewContext
    }
  | {
      kind: typeof ReviewQueueSelectionKind.CaughtUp
      nextDueAt: number | null
      nextNormalLaneCursor: number
    }

export interface RecordGradeInput {
  eventId: string
  reviewCardId: string
  expectedReviewCardUpdatedAt: number
  expectedCardContentUpdatedAt: number
  expectedCardSequence: number
  expectedSchedulerConfigId: string
  review: ReviewFact
  nextCache: ReviewCardCache
  schedulerLog: SchedulerLogV1
}

export interface UndoLastGradeInput {
  eventId: string
  reviewCardId: string
  targetEventId: string
  expectedReviewCardUpdatedAt: number
  expectedCardSequence: number
  expectedSchedulerConfigId: string
  nextCache: ReviewCardCache
}

export interface ReviewMutationResult {
  disposition: MutationDisposition
  eventId: string
  cardSequence: number
  context: ReviewContext
}

export interface UpdateCardContentInput {
  id: string
  expectedUpdatedAt: number
  content: CardContentDraft
}

export interface SearchCardContentInput {
  query: string
  limit: number
}

export interface SetCardContentSuspendedInput {
  cardContentId: string
  expectedLifecycleUpdatedAt: number
  suspended: boolean
}

export interface DeleteCardContentInput {
  cardContentId: string
  expectedUpdatedAt: number
  expectedLifecycleUpdatedAt: number
}

export interface LoadHomeStatsInput {
  now: number
  studyDay: number
  activityStartStudyDay: number
}

export interface DailyReviewActivity {
  studyDay: number
  count: number
}

export interface HomeQueueCounts {
  new: number
  learning: number
  review: number
}

export interface HomeStats {
  activity: DailyReviewActivity[]
  reviewedToday: number
  queue: HomeQueueCounts
  nextLearningDueAt: number | null
}

export interface ReviewGateway {
  selectNextReviewCard(
    input: SelectNextReviewCardInput,
  ): Promise<ReviewQueueSelection>
  recordGrade(input: RecordGradeInput): Promise<ReviewMutationResult>
  undoLastGrade(input: UndoLastGradeInput): Promise<ReviewMutationResult>
}

export interface ReviewCardView {
  lane: ReviewQueueLane
  context: ReviewContext
  selectionCursor: number
  nextNormalLaneCursor: number
}

export interface RevealedReview {
  card: ReviewCardView
  previews: GradePreview
  previewedAt: number
}

export type { ReviewCardCache, ReviewCardState, ReviewFact }
