import { invoke } from '@tauri-apps/api/core'
import type {
  CreateBasicCardInput,
  RecordGradeInput,
  ReviewContext,
  ReviewGateway,
  ReviewMutationResult,
  ReviewQueueSelection,
  SelectNextReviewCardInput,
  UndoLastGradeInput,
} from './contracts.ts'

export const tauriReviewGateway: ReviewGateway = {
  selectNextReviewCard: (input: SelectNextReviewCardInput) =>
    invoke<ReviewQueueSelection>('select_next_review_card', { input }),
  recordGrade: (input: RecordGradeInput) =>
    invoke<ReviewMutationResult>('record_grade', { input }),
  undoLastGrade: (input: UndoLastGradeInput) =>
    invoke<ReviewMutationResult>('undo_last_grade', { input }),
}

export function createBasicCard(
  input: CreateBasicCardInput,
): Promise<ReviewContext> {
  return invoke<ReviewContext>('create_basic_card', { input })
}
