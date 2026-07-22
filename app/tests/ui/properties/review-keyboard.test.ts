import { describe, expect, test } from 'vitest'
import fc from 'fast-check'
import {
  ReviewControllerPhase,
  ReviewKeyboardActionKind,
  interpretReviewKeyDown,
  interpretReviewKeyUp,
  type ReviewKeyboardInput,
} from '../../../src/review/index.ts'

const PROPERTY_SEED = Number(process.env.DARA_PROPERTY_SEED ?? Date.now())
const PROPERTY_RUNS = Number(
  process.env.DARA_PROPERTY_RUNS ??
    (process.env.DARA_PROPERTY_SEED ? 250 : 2_000),
)

console.info(`fast-check seed=${PROPERTY_SEED} runs=${PROPERTY_RUNS}`)

const boolean = fc.boolean()
const key = fc.oneof(
  fc.constantFrom(' ', 'Enter', 'Tab', '1', '2', '3', '4', 'z', 'Escape'),
  fc.string({ maxLength: 4 }),
)

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

describe('review keyboard properties', () => {
  test.each([
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { isComposing: true },
      name: 'composition',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.Undo },
      input: { canUndo: true, key: 'Z', metaKey: true },
      name: 'available undo',
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
      expected: { delta: 1, kind: ReviewKeyboardActionKind.MoveGradeFocus },
      input: { key: 'Tab', phase: ReviewControllerPhase.Revealed },
      name: 'forward grade focus',
    },
    {
      expected: { delta: -1, kind: ReviewKeyboardActionKind.MoveGradeFocus },
      input: { key: 'Tab', phase: ReviewControllerPhase.Revealed, shiftKey: true },
      name: 'backward grade focus',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: { key: 'Enter', phase: ReviewControllerPhase.Revealed, repeat: true },
      name: 'repeat after reveal',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.SubmitFocusedGrade },
      input: { key: 'Enter', phase: ReviewControllerPhase.Revealed },
      name: 'focused grade',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.SubmitFocusedGrade },
      input: { key: ' ', phase: ReviewControllerPhase.Revealed },
      name: 'armed Space grade',
    },
    {
      expected: { kind: ReviewKeyboardActionKind.None },
      input: {
        key: ' ',
        phase: ReviewControllerPhase.Revealed,
        spaceCanSubmit: false,
      },
      name: 'held Space grade',
    },
    {
      expected: { grade: 1, kind: ReviewKeyboardActionKind.DirectGrade },
      input: { key: '1', phase: ReviewControllerPhase.Revealed },
      name: 'lowest numeric grade',
    },
    {
      expected: { grade: 4, kind: ReviewKeyboardActionKind.DirectGrade },
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
  ])('$name maps to its named action', ({ expected, input }) => {
    const interpreted = interpretReviewKeyDown({ ...baseInput, ...input })
    expect(interpreted.action).toEqual(expected)
    expect(interpreted.preventDefault).toBe(
      expected.kind !== ReviewKeyboardActionKind.None,
    )
  })

  test('cannot grade before reveal for any key or modifier combination', () => {
    fc.assert(
      fc.property(key, boolean, boolean, boolean, boolean, boolean, (value, altKey, ctrlKey, metaKey, shiftKey, repeat) => {
        const result = interpretReviewKeyDown({
          ...baseInput,
          altKey,
          ctrlKey,
          key: value,
          metaKey,
          repeat,
          shiftKey,
        })
        expect([
          ReviewKeyboardActionKind.DirectGrade,
          ReviewKeyboardActionKind.SubmitFocusedGrade,
        ]).not.toContain(result.action.kind)
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })

  test('repeat and composition cannot reveal or grade', () => {
    fc.assert(
      fc.property(key, boolean, (value, composing) => {
        const result = interpretReviewKeyDown({
          ...baseInput,
          isComposing: composing,
          key: value,
          phase: ReviewControllerPhase.Revealed,
          repeat: !composing,
        })
        expect([
          ReviewKeyboardActionKind.DirectGrade,
          ReviewKeyboardActionKind.Reveal,
          ReviewKeyboardActionKind.SubmitFocusedGrade,
        ]).not.toContain(result.action.kind)
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })

  test('the Space keydown that reveals cannot also submit', () => {
    const reveal = interpretReviewKeyDown(baseInput)
    expect(reveal.action.kind).toBe(ReviewKeyboardActionKind.Reveal)
    expect(reveal.nextSpaceCanSubmit).toBe(false)
    const held = interpretReviewKeyDown({
      ...baseInput,
      phase: ReviewControllerPhase.Revealed,
      repeat: true,
      spaceCanSubmit: reveal.nextSpaceCanSubmit,
    })
    expect(held.action.kind).toBe(ReviewKeyboardActionKind.None)
    expect(interpretReviewKeyUp(' ', held.nextSpaceCanSubmit)).toBe(true)
    expect(interpretReviewKeyUp('Enter', false)).toBe(false)
  })
})
