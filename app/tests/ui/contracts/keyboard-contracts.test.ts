import { describe, expect, test } from 'vitest'
import {
  DaraSurface,
  KeyboardAction,
  KeyboardRoute,
  KEYBOARD_CONTRACTS,
} from '../../../src/lib/keyboard-contracts.ts'
import {
  ReviewControllerPhase,
  ReviewKeyboardActionKind,
  interpretReviewKeyDown,
  interpretReviewKeyUp,
  type ReviewKeyboardInput,
} from '../../../src/review/index.ts'

const baseInput: ReviewKeyboardInput = {
  altKey: false,
  canUndo: false,
  ctrlKey: false,
  isComposing: false,
  key: ' ',
  metaKey: false,
  phase: ReviewControllerPhase.Question,
  repeat: false,
  shiftKey: false,
  spaceCanSubmit: true,
}

const reviewExamples = [
  {
    action: KeyboardAction.ReviewReveal,
    expectedKind: ReviewKeyboardActionKind.Reveal,
    input: {},
  },
  {
    action: KeyboardAction.ReviewMoveGradeFocus,
    expectedKind: ReviewKeyboardActionKind.MoveGradeFocus,
    input: { key: 'Tab', phase: ReviewControllerPhase.Revealed },
  },
  {
    action: KeyboardAction.ReviewSubmitFocusedGrade,
    expectedKind: ReviewKeyboardActionKind.SubmitFocusedGrade,
    input: { key: 'Enter', phase: ReviewControllerPhase.Revealed },
  },
  {
    action: KeyboardAction.ReviewDirectGrade,
    expectedKind: ReviewKeyboardActionKind.DirectGrade,
    input: { key: '1', phase: ReviewControllerPhase.Revealed },
  },
  {
    action: KeyboardAction.ReviewUndo,
    expectedKind: ReviewKeyboardActionKind.Undo,
    input: { canUndo: true, key: 'z', metaKey: true },
  },
] as const

describe('keyboard contract registry', () => {
  test('has a contract row for every named action', () => {
    const actions = new Set(
      KEYBOARD_CONTRACTS.map((contract) => contract.action),
    )
    expect([...actions].toSorted()).toEqual(
      Object.values(KeyboardAction).toSorted(),
    )
  })

  test.each(reviewExamples)(
    '$action is wired to the review interpreter',
    ({ action, expectedKind, input }) => {
      const contract = KEYBOARD_CONTRACTS.find((row) => row.action === action)
      expect(contract).toMatchObject({
        action,
        route: KeyboardRoute.Dom,
        surface: DaraSurface.Review,
      })
      expect(
        interpretReviewKeyDown({ ...baseInput, ...input }).action.kind,
      ).toBe(expectedKind)
    },
  )

  test.each([
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { isComposing: true },
      name: 'composition',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { canUndo: false, key: 'z', metaKey: true },
      name: 'unavailable undo',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { altKey: true, canUndo: true, key: 'z', metaKey: true },
      name: 'modified undo',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { ctrlKey: true, key: 'Enter' },
      name: 'foreign modifier',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { repeat: true },
      name: 'repeat before reveal',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { key: 'Enter', phase: ReviewControllerPhase.CaughtUp },
      name: 'non-review phase',
    },
    {
      expected: {
        delta: -1,
        kind: ReviewKeyboardActionKind.MoveGradeFocus,
      },
      input: {
        key: 'Tab',
        phase: ReviewControllerPhase.Revealed,
        shiftKey: true,
      },
      name: 'backward grade focus',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: {
        key: 'Enter',
        phase: ReviewControllerPhase.Revealed,
        repeat: true,
      },
      name: 'repeat after reveal',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: {
        key: ' ',
        phase: ReviewControllerPhase.Revealed,
        spaceCanSubmit: false,
      },
      name: 'unarmed Space grade',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.SubmitFocusedGrade },
      input: { key: ' ', phase: ReviewControllerPhase.Revealed },
      name: 'armed Space grade',
    },
    {
      expected: {
        grade: 4,
        kind: ReviewKeyboardActionKind.DirectGrade,
      },
      input: { key: '4', phase: ReviewControllerPhase.Revealed },
      name: 'highest numeric grade',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { key: '1.5', phase: ReviewControllerPhase.Revealed },
      name: 'fractional numeric key',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { key: '5', phase: ReviewControllerPhase.Revealed },
      name: 'out-of-range numeric key',
    },
  ])('$name follows the registered review guards', ({ expected, input }) => {
    const interpreted = interpretReviewKeyDown({ ...baseInput, ...input })
    expect(interpreted.action).toEqual(expected)
    expect(interpreted.preventDefault).toBe(
      expected.kind !== ReviewKeyboardActionKind.None,
    )
  })

  test('Space keyup arms a later grade without other keys changing the guard', () => {
    expect(interpretReviewKeyUp(' ', false)).toBe(true)
    expect(interpretReviewKeyUp('Enter', false)).toBe(false)
  })
})
