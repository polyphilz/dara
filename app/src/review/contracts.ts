import type {
  GradePreview,
  ReviewCardCache,
  ReviewCardState,
  ReviewFact,
  SchedulerConfigV1,
  SchedulerLogV1,
} from '../scheduling/index.ts'

export type ReviewCardStatus = 'ACTIVE' | 'SUSPENDED'
export type ReviewQueueLane = 'INTRADAY' | 'REVIEW' | 'NEW'
export type MutationDisposition = 'APPLIED' | 'ALREADY_APPLIED'

export interface BasicCardContent {
  id: string
  createdAt: number
  updatedAt: number
  type: 'BASIC'
  frontMd: string
  backMd: string
  source: string | null
}

export type CardContent = BasicCardContent

export type CardContentDraft = Pick<
  BasicCardContent,
  'type' | 'frontMd' | 'backMd' | 'source'
>

export type CardContentReviewStatus = 'ACTIVE' | 'SUSPENDED' | 'MIXED'

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
      kind: 'CARD'
      lane: ReviewQueueLane
      nextNormalLaneCursor: number
      context: ReviewContext
    }
  | {
      kind: 'CAUGHT_UP'
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
