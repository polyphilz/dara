import { ReviewControllerPhase } from './controller.ts'
import type { ReviewGrade } from '../scheduling/types.ts'

export const ReviewKeyboardActionKind = {
  DirectGrade: 'DIRECT_GRADE',
  MoveGradeFocus: 'MOVE_GRADE_FOCUS',
  None: 'NONE',
  Reveal: 'REVEAL',
  SubmitFocusedGrade: 'SUBMIT_FOCUSED_GRADE',
  Undo: 'UNDO',
} as const

export type ReviewKeyboardAction =
  | { kind: typeof ReviewKeyboardActionKind.None }
  | { kind: typeof ReviewKeyboardActionKind.Reveal }
  | { kind: typeof ReviewKeyboardActionKind.SubmitFocusedGrade }
  | { kind: typeof ReviewKeyboardActionKind.DirectGrade; grade: ReviewGrade }
  | { kind: typeof ReviewKeyboardActionKind.MoveGradeFocus; delta: -1 | 1 }
  | { kind: typeof ReviewKeyboardActionKind.Undo }

export interface ReviewKeyboardInput {
  altKey: boolean
  canUndo: boolean
  ctrlKey: boolean
  isComposing: boolean
  key: string
  metaKey: boolean
  phase: ReviewControllerPhase
  repeat: boolean
  shiftKey: boolean
  spaceCanSubmit: boolean
}

export interface ReviewKeyboardResult {
  action: ReviewKeyboardAction
  nextSpaceCanSubmit: boolean
  preventDefault: boolean
}

const noAction = { kind: ReviewKeyboardActionKind.None } as const

export function interpretReviewKeyDown(
  input: ReviewKeyboardInput,
): ReviewKeyboardResult {
  if (input.isComposing) {
    return result(noAction, input.spaceCanSubmit)
  }
  if (
    input.metaKey &&
    !input.altKey &&
    !input.ctrlKey &&
    !input.shiftKey &&
    input.key.toLowerCase() === 'z'
  ) {
    return input.canUndo
      ? result({ kind: ReviewKeyboardActionKind.Undo }, input.spaceCanSubmit)
      : result(noAction, input.spaceCanSubmit)
  }
  if (input.metaKey || input.altKey || input.ctrlKey) {
    return result(noAction, input.spaceCanSubmit)
  }
  if (
    input.phase === ReviewControllerPhase.Question &&
    input.key === ' ' &&
    !input.repeat
  ) {
    return result({ kind: ReviewKeyboardActionKind.Reveal }, false)
  }
  if (input.phase !== ReviewControllerPhase.Revealed) {
    return result(noAction, input.spaceCanSubmit)
  }
  if (input.key === 'Tab') {
    return result(
      {
        kind: ReviewKeyboardActionKind.MoveGradeFocus,
        delta: input.shiftKey ? -1 : 1,
      },
      input.spaceCanSubmit,
    )
  }
  if (input.repeat) {
    return result(noAction, input.spaceCanSubmit)
  }
  if (input.key === 'Enter') {
    return result(
      { kind: ReviewKeyboardActionKind.SubmitFocusedGrade },
      input.spaceCanSubmit,
    )
  }
  if (input.key === ' ' && input.spaceCanSubmit) {
    return result(
      { kind: ReviewKeyboardActionKind.SubmitFocusedGrade },
      input.spaceCanSubmit,
    )
  }
  const numericGrade = Number(input.key)
  if (isReviewGrade(numericGrade)) {
    return result(
      { kind: ReviewKeyboardActionKind.DirectGrade, grade: numericGrade },
      input.spaceCanSubmit,
    )
  }
  return result(noAction, input.spaceCanSubmit)
}

export function interpretReviewKeyUp(
  key: string,
  spaceCanSubmit: boolean,
): boolean {
  return key === ' ' ? true : spaceCanSubmit
}

function result(
  action: ReviewKeyboardAction,
  nextSpaceCanSubmit: boolean,
): ReviewKeyboardResult {
  return {
    action,
    nextSpaceCanSubmit,
    preventDefault: action.kind !== ReviewKeyboardActionKind.None,
  }
}

function isReviewGrade(value: number): value is ReviewGrade {
  return Number.isInteger(value) && value >= 1 && value <= 4
}
