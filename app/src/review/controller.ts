import {
  captureStudyMoment,
  previewReview,
  replayReviews,
} from '../scheduling/index.ts'
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
  ReviewQueueLane,
  UndoLastGradeInput,
} from './contracts.ts'
import { commandErrorCode, errorMessage } from './errors.ts'
import { createUuidV7 } from './uuid-v7.ts'

interface StateBase {
  canUndo: boolean
}

export type ReviewControllerState =
  | (StateBase & { phase: 'IDLE' })
  | (StateBase & { phase: 'LOADING'; notice: string | null })
  | (StateBase & {
      phase: 'QUESTION'
      card: ReviewCardView
      notice: string | null
    })
  | (StateBase & {
      phase: 'REVEALED'
      card: ReviewCardView
      previews: GradePreview
      previewedAt: number
      focusedGrade: ReviewGrade
      notice: string | null
    })
  | (StateBase & {
      phase: 'SUBMITTING'
      card: ReviewCardView
      previews: GradePreview
      previewedAt: number
      focusedGrade: ReviewGrade
    })
  | (StateBase & { phase: 'UNDOING' })
  | (StateBase & {
      phase: 'CAUGHT_UP'
      nextDueAt: number | null
      notice: string | null
    })
  | (StateBase & {
      phase: 'ERROR'
      message: string
      canRetry: boolean
    })

interface UndoRecord {
  targetEventId: string
  context: ReviewContext
  selectionCursor: number
  nextNormalLaneCursor: number
}

type Listener = () => void
type RetryOperation = () => Promise<void>

export interface ReviewControllerOptions {
  captureMoment?: () => StudyMoment
  createEventId?: () => string
}

export class ReviewController {
  private readonly gateway: ReviewGateway
  private state: ReviewControllerState = { phase: 'IDLE', canUndo: false }
  private readonly listeners = new Set<Listener>()
  private readonly captureMoment: () => StudyMoment
  private readonly createEventId: () => string
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
  }

  readonly getSnapshot = (): ReviewControllerState => this.state

  readonly subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  async start(): Promise<void> {
    if (this.state.phase !== 'IDLE') {
      return
    }
    await this.loadNext(this.normalLaneCursor, null)
  }

  async refresh(): Promise<void> {
    await this.loadNext(this.normalLaneCursor, null)
  }

  notifyCardCreated(): void {
    if (this.state.phase === 'CAUGHT_UP') {
      void this.loadNext(this.normalLaneCursor, null)
    }
  }

  async notifyClockChanged(): Promise<void> {
    if (this.state.phase === 'CAUGHT_UP') {
      await this.loadNext(this.normalLaneCursor, null)
    }
  }

  reveal(): void {
    if (this.state.phase !== 'QUESTION') {
      return
    }
    const { card, notice } = this.state
    try {
      const moment = this.captureMoment()
      const previews = calculatePreviews(card.context, moment)
      this.publish({
        phase: 'REVEALED',
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
          phase: 'QUESTION',
          card,
          notice,
          canUndo: this.lastGrade !== null,
        })
        this.reveal()
      })
    }
  }

  moveGradeFocus(direction: -1 | 1): void {
    if (this.state.phase !== 'REVEALED') {
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
    if (this.state.phase !== 'REVEALED') {
      return
    }
    await this.submitGrade(this.state.focusedGrade)
  }

  async submitGrade(grade: ReviewGrade): Promise<void> {
    if (this.state.phase !== 'REVEALED') {
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
      !['QUESTION', 'REVEALED', 'CAUGHT_UP'].includes(this.state.phase)
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
    if (this.state.phase !== 'ERROR' || retry === null) {
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
      phase: 'SUBMITTING',
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
      this.normalLaneCursor = card.nextNormalLaneCursor
      await this.loadNext(this.normalLaneCursor, null)
    } catch (error) {
      if (operationId !== this.operationId) {
        return
      }
      if (commandErrorCode(error) === 'staleReviewContext') {
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
    this.publish({ phase: 'UNDOING', canUndo: true })
    try {
      const result = await this.gateway.undoLastGrade(input)
      if (operationId !== this.operationId) {
        return
      }
      this.lastGrade = null
      this.normalLaneCursor = undoRecord.nextNormalLaneCursor
      this.publish({
        phase: 'QUESTION',
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
      if (commandErrorCode(error) === 'staleReviewContext') {
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

  private async loadNext(cursor: number, notice: string | null): Promise<void> {
    const operationId = ++this.operationId
    this.retryOperation = null
    this.publish({
      phase: 'LOADING',
      notice,
      canUndo: this.lastGrade !== null,
    })
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
      if (result.kind === 'CAUGHT_UP') {
        this.publish({
          phase: 'CAUGHT_UP',
          nextDueAt: result.nextDueAt,
          notice,
          canUndo: this.lastGrade !== null,
        })
        return
      }
      this.publish({
        phase: 'QUESTION',
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
      phase: 'ERROR',
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
    case 'NEW':
      return 'NEW'
    case 'LEARNING':
    case 'RELEARNING':
      return 'INTRADAY'
    case 'REVIEW':
      return 'REVIEW'
  }
}
