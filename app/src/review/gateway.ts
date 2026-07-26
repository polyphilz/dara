import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
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
    invoke<ReviewQueueSelection>(DaraIpcCommand.SelectNextReviewCard, { input }),
  recordGrade: (input: RecordGradeInput) =>
    invoke<ReviewMutationResult>(DaraIpcCommand.RecordGrade, { input }),
  undoLastGrade: (input: UndoLastGradeInput) =>
    invoke<ReviewMutationResult>(DaraIpcCommand.UndoLastGrade, { input }),
}

export function createCardContent(
  input: CardContentDraft,
  mediaLeaseId: string,
): Promise<ReviewContext> {
  return invoke<ReviewContext>(DaraIpcCommand.CreateCardContent, { input, mediaLeaseId })
}

export function updateCardContent(
  input: UpdateCardContentInput,
  mediaLeaseId: string,
): Promise<CardContentListItem> {
  return invoke<CardContentListItem>(DaraIpcCommand.UpdateCardContent, {
    input,
    mediaLeaseId,
  })
}

export function loadCardContent(
  cardContentId: string,
): Promise<CardContentListItem> {
  return invoke<CardContentListItem>(DaraIpcCommand.LoadCardContent, {
    cardContentId,
  })
}

export function searchCardContent(
  input: SearchCardContentInput,
): Promise<SearchCardContentResult> {
  return invoke<SearchCardContentResult>(DaraIpcCommand.SearchCardContent, { input })
}

export function searchStatus(): Promise<SemanticSearchStatus> {
  return invoke<SemanticSearchStatus>(DaraIpcCommand.SearchStatus)
}

export function maintainSearch(
  operation: SearchMaintenanceOperation,
): Promise<SearchMaintenanceReport> {
  return invoke<SearchMaintenanceReport>(DaraIpcCommand.MaintainSearch, { operation })
}

export function setCardContentSuspended(
  input: SetCardContentSuspendedInput,
): Promise<CardContentListItem> {
  return invoke<CardContentListItem>(DaraIpcCommand.SetCardContentSuspended, { input })
}

export function deleteCardContent(
  input: DeleteCardContentInput,
): Promise<void> {
  return invoke<void>(DaraIpcCommand.DeleteCardContent, { input })
}

export function loadHomeStats(
  input: LoadHomeStatsInput,
): Promise<HomeStats> {
  return invoke<HomeStats>(DaraIpcCommand.LoadHomeStats, { input })
}
