import { describe, expect, test } from 'vitest'
import { render, screen } from '@testing-library/react'
import { createElement } from 'react'
import { ClozeMarkdownRenderer } from '../../../src/cloze/ClozeMarkdownRenderer.tsx'
import {
  ClozeParseErrorCode,
  ClozeProjection,
  clozeAnswerMarkdown,
  clozeIndexFromVariantKey,
  clozeQuestionMarkdown,
  parseClozeMarkdown,
  projectClozeMarkdown,
} from '../../../src/cloze/cloze.ts'

describe('cloze parsing', () => {
  test('supports hints, repeated indices, escaping, and numeric variant order', () => {
    const source = [
      'The {{c10::tenth}} item follows the {{c2::second::position}}.',
      'Repeat {{c2::two}} and keep {{c1::A\\:\\:B}} together.',
      String.raw`Literal \{\{c4::not a cloze\}\}.`,
    ].join('\n')
    const document = parseClozeMarkdown(source)

    expect(document.indices).toEqual(['1', '2', '10'])
    expect(document.variantKeys).toEqual(['cloze:1', 'cloze:2', 'cloze:10'])
    expect(document.occurrences.map(({ answerMarkdown, hintMarkdown, index }) => ({
      answerMarkdown,
      hintMarkdown,
      index,
    }))).toEqual([
      { answerMarkdown: 'tenth', hintMarkdown: null, index: '10' },
      { answerMarkdown: 'second', hintMarkdown: 'position', index: '2' },
      { answerMarkdown: 'two', hintMarkdown: null, index: '2' },
      { answerMarkdown: 'A\\:\\:B', hintMarkdown: null, index: '1' },
    ])
  })

  test('leaves inline and fenced code literal while allowing Markdown in answers', () => {
    const source = [
      '`{{c8::inline literal}}`',
      'A {{c1::**formatted** and `a::b`::useful hint}} answer.',
      '```text',
      '{{c9::fenced literal}}',
      '```',
    ].join('\n')
    const document = parseClozeMarkdown(source)

    expect(document.indices).toEqual(['1'])
    expect(document.occurrences[0]).toMatchObject({
      answerMarkdown: '**formatted** and `a::b`',
      hintMarkdown: 'useful hint',
    })
    expect(
      projectClozeMarkdown(document, ClozeProjection.Question, '1'),
    ).toBe([
      '`{{c8::inline literal}}`',
      'A \\[useful hint\\] answer.',
      '```text',
      '{{c9::fenced literal}}',
      '```',
    ].join('\n'))
  })

  test('projects only the selected question index and reveals every answer', () => {
    const source = 'A {{c1::one}} plus {{c2::two::second}} and {{c1::uno}}.'

    expect(clozeQuestionMarkdown(source, 'cloze:1')).toBe(
      'A \\[...\\] plus two and \\[...\\].',
    )
    expect(clozeQuestionMarkdown(source, 'cloze:2')).toBe(
      'A one plus \\[second\\] and uno.',
    )
    expect(clozeAnswerMarkdown(source)).toBe('A one plus two and uno.')
    expect(clozeAnswerMarkdown(String.raw`{{c1::A\:\:B}}`)).toBe('A::B')
    expect(clozeIndexFromVariantKey('cloze:200')).toBe('200')
  })

  test.each([
    ['plain text', ClozeParseErrorCode.MissingCloze],
    ['{{c0::answer}}', ClozeParseErrorCode.InvalidIndex],
    ['{{c01::answer}}', ClozeParseErrorCode.InvalidIndex],
    ['{{C1::answer}}', ClozeParseErrorCode.InvalidHeader],
    ['{{c1:answer}}', ClozeParseErrorCode.InvalidHeader],
    ['{{c1:: }}', ClozeParseErrorCode.EmptyAnswer],
    ['{{c1::outer {{c2::inner}}}}', ClozeParseErrorCode.NestedCloze],
    ['{{c1::answer::hint::extra}}', ClozeParseErrorCode.TooManyFields],
    ['{{c1::answer', ClozeParseErrorCode.UnclosedCloze],
    ['answer }}', ClozeParseErrorCode.UnexpectedClose],
  ])('rejects invalid source %s', (source, code) => {
    expect(() => parseClozeMarkdown(source)).toThrowError(
      expect.objectContaining({ code }),
    )
  })

  test('rejects a selected variant that does not exist', () => {
    expect(() => clozeQuestionMarkdown('{{c1::one}}', 'cloze:2')).toThrowError(
      expect.objectContaining({
        code: ClozeParseErrorCode.UnknownVariant,
      }),
    )
  })

  test('renders the selected question and fully revealed answer as Markdown', () => {
    const source =
      'The {{c1::**capital**}} of France is {{c2::Paris::city}}. `{{c9::literal}}`'
    const { container, rerender } = render(
      createElement(ClozeMarkdownRenderer, {
        projection: ClozeProjection.Question,
        source,
        variantKey: 'cloze:2',
      }),
    )

    expect(container.textContent).toContain('The capital of France is [city].')
    expect(container.textContent).not.toContain('Paris')
    expect(container.textContent).toContain('{{c9::literal}}')
    expect(container.querySelector('strong')?.textContent).toBe('capital')
    const placeholder = screen.getByRole('note', {
      name: 'Hidden cloze deletion',
    })
    expect(placeholder.classList.contains('dara-cloze-placeholder')).toBe(true)
    expect(placeholder.textContent).toBe('[city]')
    expect(container.querySelector('a')).toBeNull()

    rerender(createElement(ClozeMarkdownRenderer, {
      projection: ClozeProjection.Question,
      source,
      variantKey: 'cloze:1',
    }))
    expect(
      screen.getByRole('note', { name: 'Hidden cloze deletion' }).textContent,
    ).toBe('[...]')

    rerender(createElement(ClozeMarkdownRenderer, {
      projection: ClozeProjection.Answer,
      source,
    }))
    expect(container.textContent).toContain(
      'The capital of France is Paris. {{c9::literal}}',
    )
    expect(container.textContent).not.toContain('city')
    expect(
      screen.queryByRole('note', { name: 'Hidden cloze deletion' }),
    ).toBeNull()
  })

  test('keeps ordinary links distinct from a question placeholder', () => {
    const source =
      'Read [the notes](https://example.com) about {{c1::Paris::the city}}.'
    render(createElement(ClozeMarkdownRenderer, {
      projection: ClozeProjection.Question,
      source,
      variantKey: 'cloze:1',
    }))

    expect(
      screen.getByRole('link', { name: 'the notes' }).getAttribute('href'),
    ).toBe('https://example.com/')
    expect(
      screen.getByRole('note', { name: 'Hidden cloze deletion' }).textContent,
    ).toBe('[the city]')
  })
})
