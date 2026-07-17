import type {
  GradePreview,
  ReviewCardCache,
  ReviewCardState,
  ReviewFact,
  SchedulerConfigV1,
  SchedulerLogV1,
} from '../scheduling/index.ts'
import type { ImageRecord } from '../media/image-reference.ts'

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
  Cloze: 'CLOZE',
  Occlusion: 'OCCLUSION',
} as const

export type CardContentType =
  (typeof CardContentType)[keyof typeof CardContentType]

interface CardContentBase {
  id: string
  createdAt: number
  updatedAt: number
  frontMd: string
  backMd: string
  source: string | null
}

export interface BasicCardContent extends CardContentBase {
  type: typeof CardContentType.Basic
}

export interface ClozeCardContent extends CardContentBase {
  type: typeof CardContentType.Cloze
}

export const OcclusionMode = {
  HideOneGuessOne: 'HIDE_ONE_GUESS_ONE',
  HideAllGuessOne: 'HIDE_ALL_GUESS_ONE',
} as const

export type OcclusionMode =
  (typeof OcclusionMode)[keyof typeof OcclusionMode]

export const OcclusionMaskColor = {
  White: 'WHITE',
  Black: 'BLACK',
} as const

export type OcclusionMaskColor =
  (typeof OcclusionMaskColor)[keyof typeof OcclusionMaskColor]

export const OcclusionLayerVariantPrefix = 'layer:'

export type OcclusionSourceImage = ImageRecord

export interface OcclusionMask {
  id: string
  x: number
  y: number
  width: number
  height: number
  color: OcclusionMaskColor
}

export interface OcclusionMaskLayer {
  id: string
  label: string | null
  masks: OcclusionMask[]
}

export interface OcclusionDefinition {
  id: string
  sourceImage: OcclusionSourceImage
  mode: OcclusionMode
  layers: OcclusionMaskLayer[]
}

export interface OcclusionCardContent extends CardContentBase {
  type: typeof CardContentType.Occlusion
  occlusion: OcclusionDefinition
}

export type CardContent =
  | BasicCardContent
  | ClozeCardContent
  | OcclusionCardContent

export type BasicCardContentDraft = Pick<
  BasicCardContent,
  'type' | 'frontMd' | 'backMd' | 'source'
>

export type ClozeCardContentDraft = Pick<
  ClozeCardContent,
  'type' | 'frontMd' | 'backMd' | 'source'
> & {
  searchMd: string
  variantKeys: string[]
}

export interface OcclusionMaskDraft extends OcclusionMask {}

export interface OcclusionMaskLayerDraft extends OcclusionMaskLayer {}

export interface OcclusionDefinitionDraft {
  id: string
  sourceImageId: string
  mode: OcclusionMode
  layers: OcclusionMaskLayerDraft[]
}

export type OcclusionCardContentDraft = Pick<
  OcclusionCardContent,
  'type' | 'frontMd' | 'backMd' | 'source'
> & {
  occlusion: OcclusionDefinitionDraft
}

export type CardContentDraft =
  | BasicCardContentDraft
  | ClozeCardContentDraft
  | OcclusionCardContentDraft

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
  reviewCards: ReviewCardListItem[]
  reviewStatus: CardContentReviewStatus
  lifecycleUpdatedAt: number
}

export interface ReviewCardListItem {
  id: string
  status: ReviewCardStatus
  variantKey: string
  state: ReviewCardState
  dueAt: number | null
  dueStudyDay: number | null
  lastReviewAt: number | null
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
  offset: number
}

export const SemanticSearchPhase = {
  Downloading: 'DOWNLOADING',
  Verifying: 'VERIFYING',
  Starting: 'STARTING',
  Indexing: 'INDEXING',
  Ready: 'READY',
  Unavailable: 'UNAVAILABLE',
  Failed: 'FAILED',
} as const

export type SemanticSearchPhase =
  (typeof SemanticSearchPhase)[keyof typeof SemanticSearchPhase]

export const SearchExecutionMode = {
  Browse: 'BROWSE',
  Lexical: 'LEXICAL',
  Hybrid: 'HYBRID',
} as const

export type SearchExecutionMode =
  (typeof SearchExecutionMode)[keyof typeof SearchExecutionMode]

export interface SemanticSearchStatus {
  phase: SemanticSearchPhase
  downloadedBytes: number
  modelBytes: number
  indexedDocuments: number
  totalDocuments: number
  message: string | null
}

export interface SearchCardContentResult {
  items: CardContentListItem[]
  mode: SearchExecutionMode
  semanticStatus: SemanticSearchStatus
}

export const SearchMaintenanceOperation = {
  IntegrityCheck: 'INTEGRITY_CHECK',
  RebuildFts: 'REBUILD_FTS',
} as const

export type SearchMaintenanceOperation =
  (typeof SearchMaintenanceOperation)[keyof typeof SearchMaintenanceOperation]

export interface SearchMaintenanceReport {
  operation: SearchMaintenanceOperation
  searchDocuments: number
  ftsRows: number
  indexedDocuments: number
  totalEmbeddingDocuments: number
  semanticIndexActive: boolean
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
