import {
  captureStudyMoment,
  previewReview,
  replayReviews,
  ReviewCardState,
} from '../scheduling/index.ts'
import {
  ReviewQueueLane,
  ReviewQueueSelectionKind,
} from './contracts.ts'
import type {
  GradePreview,
  ReviewGrade,
  StudyMoment,
} from '../scheduling/index.ts'
import type {
  RecordGradeInput,
  ReviewCardView,
  ReviewContext,
  ReviewGateway,
  UndoLastGradeInput,
} from './contracts.ts'
import {
  CommandErrorCode,
  commandErrorCode,
  errorMessage,
} from './errors.ts'
import { createUuidV7 } from './uuid-v7.ts'

interface StateBase {
  canUndo: boolean
}

export const ReviewControllerPhase = {
  Idle: 'IDLE',
  Loading: 'LOADING',
  Question: 'QUESTION',
  Revealed: 'REVEALED',
  Submitting: 'SUBMITTING',
  Undoing: 'UNDOING',
  CaughtUp: 'CAUGHT_UP',
  Error: 'ERROR',
} as const

export type ReviewControllerPhase =
  (typeof ReviewControllerPhase)[keyof typeof ReviewControllerPhase]

export type ReviewControllerState =
  | (StateBase & { phase: typeof ReviewControllerPhase.Idle })
  | (StateBase & {
      phase: typeof ReviewControllerPhase.Loading
      notice: string | null
    })
  | (StateBase & {
      phase: typeof ReviewControllerPhase.Question
      card: ReviewCardView
      notice: string | null
    })
  | (StateBase & {
      phase: typeof ReviewControllerPhase.Revealed
      card: ReviewCardView
      previews: GradePreview
      previewedAt: number
      focusedGrade: ReviewGrade
      notice: string | null
    })
  | (StateBase & {
      phase: typeof ReviewControllerPhase.Submitting
      card: ReviewCardView
      previews: GradePreview
      previewedAt: number
      focusedGrade: ReviewGrade
    })
  | (StateBase & { phase: typeof ReviewControllerPhase.Undoing })
  | (StateBase & {
      phase: typeof ReviewControllerPhase.CaughtUp
      nextDueAt: number | null
      notice: string | null
    })
  | (StateBase & {
      phase: typeof ReviewControllerPhase.Error
      message: string
      canRetry: boolean
    })

interface UndoRecord {
  targetEventId: string
  context: ReviewContext
  selectionCursor: number
  nextNormalLaneCursor: number
}

interface LoadNextOptions {
  retainCurrentState?: boolean
}

type Listener = () => void
type RetryOperation = () => Promise<void>

export interface ReviewControllerOptions {
  captureMoment?: () => StudyMoment
  createEventId?: () => string
  onReviewDataChanged?: () => void
}

export class ReviewController {
  private readonly gateway: ReviewGateway
  private state: ReviewControllerState = {
    phase: ReviewControllerPhase.Idle,
    canUndo: false,
  }
  private readonly listeners = new Set<Listener>()
  private readonly captureMoment: () => StudyMoment
  private readonly createEventId: () => string
  private readonly onReviewDataChanged: () => void
  private normalLaneCursor = 0
  private lastGrade: UndoRecord | null = null
  private retryOperation: RetryOperation | null = null
  private operationId = 0

  constructor(
    gateway: ReviewGateway,
    options: ReviewControllerOptions = {},
  ) {
    this.gateway = gateway
    this.captureMoment =
      options.captureMoment ?? (() => captureStudyMoment(Date.now()))
    this.createEventId = options.createEventId ?? (() => createUuidV7())
    this.onReviewDataChanged = options.onReviewDataChanged ?? (() => undefined)
  }

  readonly getSnapshot = (): ReviewControllerState => this.state

  readonly subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  async start(): Promise<void> {
    if (this.state.phase !== ReviewControllerPhase.Idle) {
      return
    }
    await this.loadNext(this.normalLaneCursor, null)
  }

  async refresh(): Promise<void> {
    await this.loadNext(this.normalLaneCursor, null)
  }

  notifyCardCreated(): void {
    if (this.state.phase === ReviewControllerPhase.CaughtUp) {
      void this.loadNext(this.normalLaneCursor, null)
    }
  }

  async notifyClockChanged(): Promise<void> {
    if (this.state.phase === ReviewControllerPhase.CaughtUp) {
      await this.loadNext(this.normalLaneCursor, null)
    }
  }

  reveal(): void {
    if (this.state.phase !== ReviewControllerPhase.Question) {
      return
    }
    const { card, notice } = this.state
    try {
      const moment = this.captureMoment()
      const previews = calculatePreviews(card.context, moment)
      this.publish({
        phase: ReviewControllerPhase.Revealed,
        card,
        previews,
        previewedAt: moment.reviewedAt,
        focusedGrade: 3,
        notice,
        canUndo: this.lastGrade !== null,
      })
    } catch (error) {
      this.fail(error, async () => {
        this.publish({
          phase: ReviewControllerPhase.Question,
          card,
          notice,
          canUndo: this.lastGrade !== null,
        })
        this.reveal()
      })
    }
  }

  moveGradeFocus(direction: -1 | 1): void {
    if (this.state.phase !== ReviewControllerPhase.Revealed) {
      return
    }
    const focusedGrade = Math.min(
      4,
      Math.max(1, this.state.focusedGrade + direction),
    ) as ReviewGrade
    if (focusedGrade !== this.state.focusedGrade) {
      this.publish({ ...this.state, focusedGrade })
    }
  }

  async submitFocusedGrade(): Promise<void> {
    if (this.state.phase !== ReviewControllerPhase.Revealed) {
      return
    }
    await this.submitGrade(this.state.focusedGrade)
  }

  async submitGrade(grade: ReviewGrade): Promise<void> {
    if (this.state.phase !== ReviewControllerPhase.Revealed) {
      return
    }
    const revealed = this.state
    const gradeMoment = this.captureMoment()
    const gradePreviews = calculatePreviews(revealed.card.context, gradeMoment)
    const transition = gradePreviews[grade]
    const input = recordGradeInput(
      revealed.card.context,
      gradeMoment,
      grade,
      transition.cache,
      transition.schedulerLog,
      this.createEventId(),
    )
    await this.persistGrade(
      revealed.card,
      revealed.previews,
      revealed.previewedAt,
      grade,
      input,
    )
  }

  async undo(): Promise<void> {
    if (
      this.lastGrade === null ||
      !undoablePhases.has(this.state.phase)
    ) {
      return
    }
    const undoRecord = this.lastGrade
    const target = undoRecord.context.reviewHistory.at(-1)
    if (target?.eventId !== undoRecord.targetEventId) {
      this.lastGrade = null
      this.fail(new Error('The last grade is no longer available to undo'), null)
      return
    }

    const remainingReviews = undoRecord.context.reviewHistory
      .slice(0, -1)
      .map((event) => event.review)
    const nextCache = replayReviews(
      undoRecord.context.reviewCard.id,
      remainingReviews,
      undoRecord.context.schedulerConfig,
    ).cache
    const input: UndoLastGradeInput = {
      eventId: this.createEventId(),
      reviewCardId: undoRecord.context.reviewCard.id,
      targetEventId: undoRecord.targetEventId,
      expectedReviewCardUpdatedAt: undoRecord.context.reviewCard.updatedAt,
      expectedCardSequence: undoRecord.context.lastCardSequence,
      expectedSchedulerConfigId: undoRecord.context.schedulerConfig.id,
      nextCache,
    }
    await this.persistUndo(undoRecord, input)
  }

  async retry(): Promise<void> {
    const retry = this.retryOperation
    if (this.state.phase !== ReviewControllerPhase.Error || retry === null) {
      return
    }
    this.retryOperation = null
    await retry()
  }

  private async persistGrade(
    card: ReviewCardView,
    previews: GradePreview,
    previewedAt: number,
    grade: ReviewGrade,
    input: RecordGradeInput,
  ): Promise<void> {
    const operationId = ++this.operationId
    this.publish({
      phase: ReviewControllerPhase.Submitting,
      card,
      previews,
      previewedAt,
      focusedGrade: grade,
      canUndo: this.lastGrade !== null,
    })
    try {
      const result = await this.gateway.recordGrade(input)
      if (operationId !== this.operationId) {
        return
      }
      this.lastGrade = {
        targetEventId: result.eventId,
        context: result.context,
        selectionCursor: card.selectionCursor,
        nextNormalLaneCursor: card.nextNormalLaneCursor,
      }
      this.onReviewDataChanged()
      this.normalLaneCursor = card.nextNormalLaneCursor
      await this.loadNext(this.normalLaneCursor, null, {
        retainCurrentState: true,
      })
    } catch (error) {
      if (operationId !== this.operationId) {
        return
      }
      if (commandErrorCode(error) === CommandErrorCode.StaleReviewContext) {
        this.normalLaneCursor = card.selectionCursor
        await this.loadNext(
          card.selectionCursor,
          'The card changed before the grade was saved. Please reveal and grade it again.',
        )
        return
      }
      this.fail(error, () =>
        this.persistGrade(card, previews, previewedAt, grade, input),
      )
    }
  }

  private async persistUndo(
    undoRecord: UndoRecord,
    input: UndoLastGradeInput,
  ): Promise<void> {
    const operationId = ++this.operationId
    this.publish({ phase: ReviewControllerPhase.Undoing, canUndo: true })
    try {
      const result = await this.gateway.undoLastGrade(input)
      if (operationId !== this.operationId) {
        return
      }
      this.lastGrade = null
      this.onReviewDataChanged()
      this.normalLaneCursor = undoRecord.nextNormalLaneCursor
      this.publish({
        phase: ReviewControllerPhase.Question,
        card: {
          lane: laneForContext(result.context),
          context: result.context,
          selectionCursor: undoRecord.selectionCursor,
          nextNormalLaneCursor: undoRecord.nextNormalLaneCursor,
        },
        notice: 'Last grade undone.',
        canUndo: false,
      })
    } catch (error) {
      if (operationId !== this.operationId) {
        return
      }
      if (commandErrorCode(error) === CommandErrorCode.StaleReviewContext) {
        this.lastGrade = null
        await this.loadNext(
          this.normalLaneCursor,
          'The graded card changed, so it could not be undone.',
        )
        return
      }
      this.fail(error, () => this.persistUndo(undoRecord, input))
    }
  }

  private async loadNext(
    cursor: number,
    notice: string | null,
    { retainCurrentState = false }: LoadNextOptions = {},
  ): Promise<void> {
    const operationId = ++this.operationId
    this.retryOperation = null
    if (!retainCurrentState) {
      this.publish({
        phase: ReviewControllerPhase.Loading,
        notice,
        canUndo: this.lastGrade !== null,
      })
    }
    try {
      const moment = this.captureMoment()
      const result = await this.gateway.selectNextReviewCard({
        now: moment.reviewedAt,
        studyDay: moment.studyDay,
        normalLaneCursor: cursor,
      })
      if (operationId !== this.operationId) {
        return
      }
      this.normalLaneCursor = result.nextNormalLaneCursor
      if (result.kind === ReviewQueueSelectionKind.CaughtUp) {
        this.publish({
          phase: ReviewControllerPhase.CaughtUp,
          nextDueAt: result.nextDueAt,
          notice,
          canUndo: this.lastGrade !== null,
        })
        return
      }
      this.publish({
        phase: ReviewControllerPhase.Question,
        card: {
          lane: result.lane,
          context: result.context,
          selectionCursor: cursor,
          nextNormalLaneCursor: result.nextNormalLaneCursor,
        },
        notice,
        canUndo: this.lastGrade !== null,
      })
    } catch (error) {
      if (operationId !== this.operationId) {
        return
      }
      this.fail(error, () => this.loadNext(cursor, notice))
    }
  }

  private fail(error: unknown, retry: RetryOperation | null): void {
    this.retryOperation = retry
    this.publish({
      phase: ReviewControllerPhase.Error,
      message: errorMessage(error),
      canRetry: retry !== null,
      canUndo: this.lastGrade !== null,
    })
  }

  private publish(state: ReviewControllerState): void {
    this.state = state
    for (const listener of this.listeners) {
      listener()
    }
  }
}

function calculatePreviews(
  context: ReviewContext,
  moment: StudyMoment,
): GradePreview {
  const previous = context.reviewHistory.at(-1)?.review
  return previewReview({
    cardId: context.reviewCard.id,
    cache: context.cache,
    previousReview: previous
      ? { reviewedAt: previous.reviewedAt, studyDay: previous.studyDay }
      : null,
    moment,
    config: context.schedulerConfig,
  })
}

function recordGradeInput(
  context: ReviewContext,
  moment: StudyMoment,
  grade: ReviewGrade,
  nextCache: RecordGradeInput['nextCache'],
  schedulerLog: RecordGradeInput['schedulerLog'],
  eventId: string,
): RecordGradeInput {
  return {
    eventId,
    reviewCardId: context.reviewCard.id,
    expectedReviewCardUpdatedAt: context.reviewCard.updatedAt,
    expectedCardContentUpdatedAt: context.cardContent.updatedAt,
    expectedCardSequence: context.lastCardSequence,
    expectedSchedulerConfigId: context.schedulerConfig.id,
    review: { ...moment, grade },
    nextCache,
    schedulerLog,
  }
}

function laneForContext(context: ReviewContext): ReviewQueueLane {
  switch (context.cache.state) {
    case ReviewCardState.New:
      return ReviewQueueLane.New
    case ReviewCardState.Learning:
    case ReviewCardState.Relearning:
      return ReviewQueueLane.Intraday
    case ReviewCardState.Review:
      return ReviewQueueLane.Review
  }
}

const undoablePhases = new Set<ReviewControllerPhase>([
  ReviewControllerPhase.Question,
  ReviewControllerPhase.Revealed,
  ReviewControllerPhase.CaughtUp,
])
