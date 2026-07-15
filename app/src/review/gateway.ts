import { invoke } from '@tauri-apps/api/core'
import type {
  CardContentDraft,
  CardContentListItem,
  DeleteCardContentInput,
  RecordGradeInput,
  ReviewContext,
  ReviewGateway,
  ReviewMutationResult,
  ReviewQueueSelection,
  SelectNextReviewCardInput,
  SearchCardContentInput,
  SetCardContentSuspendedInput,
  UndoLastGradeInput,
  UpdateCardContentInput,
} from './contracts.ts'

export const tauriReviewGateway: ReviewGateway = {
  selectNextReviewCard: (input: SelectNextReviewCardInput) =>
    invoke<ReviewQueueSelection>('select_next_review_card', { input }),
  recordGrade: (input: RecordGradeInput) =>
    invoke<ReviewMutationResult>('record_grade', { input }),
  undoLastGrade: (input: UndoLastGradeInput) =>
    invoke<ReviewMutationResult>('undo_last_grade', { input }),
}

export function createCardContent(
  input: CardContentDraft,
): Promise<ReviewContext> {
  return invoke<ReviewContext>('create_card_content', { input })
}

export function updateCardContent(
  input: UpdateCardContentInput,
): Promise<CardContentListItem> {
  return invoke<CardContentListItem>('update_card_content', { input })
}

export function searchCardContent(
  input: SearchCardContentInput,
): Promise<CardContentListItem[]> {
  return invoke<CardContentListItem[]>('search_card_content', { input })
}

export function setCardContentSuspended(
  input: SetCardContentSuspendedInput,
): Promise<CardContentListItem> {
  return invoke<CardContentListItem>('set_card_content_suspended', { input })
}

export function deleteCardContent(
  input: DeleteCardContentInput,
): Promise<void> {
  return invoke<void>('delete_card_content', { input })
}
