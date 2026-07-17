import { invoke } from '@tauri-apps/api/core'
import type {
  CardContentDraft,
  CardContentListItem,
  DeleteCardContentInput,
  HomeStats,
  LoadHomeStatsInput,
  RecordGradeInput,
  ReviewContext,
  ReviewGateway,
  ReviewMutationResult,
  ReviewQueueSelection,
  SelectNextReviewCardInput,
  SearchCardContentInput,
  SearchCardContentResult,
  SearchMaintenanceOperation,
  SearchMaintenanceReport,
  SemanticSearchStatus,
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
  mediaLeaseId: string,
): Promise<ReviewContext> {
  return invoke<ReviewContext>('create_card_content', { input, mediaLeaseId })
}

export function updateCardContent(
  input: UpdateCardContentInput,
  mediaLeaseId: string,
): Promise<CardContentListItem> {
  return invoke<CardContentListItem>('update_card_content', {
    input,
    mediaLeaseId,
  })
}

export function searchCardContent(
  input: SearchCardContentInput,
): Promise<SearchCardContentResult> {
  return invoke<SearchCardContentResult>('search_card_content', { input })
}

export function searchStatus(): Promise<SemanticSearchStatus> {
  return invoke<SemanticSearchStatus>('search_status')
}

export function maintainSearch(
  operation: SearchMaintenanceOperation,
): Promise<SearchMaintenanceReport> {
  return invoke<SearchMaintenanceReport>('maintain_search', { operation })
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

export function loadHomeStats(
  input: LoadHomeStatsInput,
): Promise<HomeStats> {
  return invoke<HomeStats>('load_home_stats', { input })
}
