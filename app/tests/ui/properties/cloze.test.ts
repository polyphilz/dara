import { describe, expect, test } from 'vitest'
import fc from 'fast-check'
import {
  clozeAnswerMarkdown,
  clozeQuestionMarkdown,
  clozeVariantKey,
  parseClozeMarkdown,
} from '../../../src/cloze/cloze.ts'

const PROPERTY_SEED = Number(process.env.DARA_PROPERTY_SEED ?? Date.now())
const PROPERTY_RUNS = Number(
  process.env.DARA_PROPERTY_RUNS ??
    (process.env.DARA_PROPERTY_SEED ? 250 : 2_000),
)

const safeText = fc
  .string({ minLength: 1, maxLength: 24 })
  .filter((value) => !/[{}:`\\]/u.test(value) && value.trim().length > 0)

describe('cloze properties', () => {
  test('question projection hides only the selected answer and preserves code', () => {
    fc.assert(
      fc.property(safeText, safeText, safeText, (answer, otherAnswer, code) => {
        fc.pre(answer !== otherAnswer)
        const source = `Before \`{{c9::${code}}}\` {{c1::${answer}}} {{c2::${otherAnswer}}}`
        const question = clozeQuestionMarkdown(source, clozeVariantKey('1'))

        expect(question).not.toContain(`{{c1::${answer}}}`)
        expect(question).toContain('\\[...\\]')
        expect(question).toContain(otherAnswer)
        expect(question).toContain(`\`{{c9::${code}}}\``)
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })

  test('answer projection removes delimiters without altering code spans', () => {
    fc.assert(
      fc.property(safeText, safeText, (answer, code) => {
        const source = `\`{{c8::${code}}}\` and {{c1::${answer}}}`
        const projected = clozeAnswerMarkdown(source)

        expect(projected).toBe(`\`{{c8::${code}}}\` and ${answer}`)
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })

  test('variant ordering is canonical regardless of source order', () => {
    fc.assert(
      fc.property(
        fc.uniqueArray(fc.integer({ min: 1, max: 999 }), {
          minLength: 1,
          maxLength: 12,
        }),
        (indices) => {
          const source = indices.map((index) => `{{c${index}::answer ${index}}}`).join(' ')
          const document = parseClozeMarkdown(source)
          expect(document.indices).toEqual(
            [...indices].sort((left, right) => left - right).map(String),
          )
        },
      ),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })
})
