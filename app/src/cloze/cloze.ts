import type { Nodes, Root } from 'mdast'
import { parseDaraMarkdownAst } from '../markdown/markdown-conversion.ts'

const CLOZE_OPEN = '{{'
const CLOZE_CLOSE = '}}'
const CLOZE_FIELD_SEPARATOR = '::'
const CLOZE_VARIANT_PREFIX = 'cloze:'
const DEFAULT_QUESTION_PLACEHOLDER = '...'

export type ClozeQuestionPlaceholderRenderer = (
  placeholderMarkdown: string,
) => string

export const ClozeParseErrorCode = {
  EmptyAnswer: 'EMPTY_ANSWER',
  InvalidHeader: 'INVALID_HEADER',
  InvalidIndex: 'INVALID_INDEX',
  MissingCloze: 'MISSING_CLOZE',
  NestedCloze: 'NESTED_CLOZE',
  TooManyFields: 'TOO_MANY_FIELDS',
  UnclosedCloze: 'UNCLOSED_CLOZE',
  UnexpectedClose: 'UNEXPECTED_CLOSE',
  UnknownVariant: 'UNKNOWN_VARIANT',
} as const

export type ClozeParseErrorCode =
  (typeof ClozeParseErrorCode)[keyof typeof ClozeParseErrorCode]

export class ClozeParseError extends Error {
  readonly code: ClozeParseErrorCode
  readonly offset: number | null

  constructor(
    code: ClozeParseErrorCode,
    message: string,
    offset: number | null = null,
  ) {
    super(message)
    this.name = 'ClozeParseError'
    this.code = code
    this.offset = offset
  }
}

export interface ClozeOccurrence {
  answerMarkdown: string
  end: number
  hintMarkdown: string | null
  index: string
  start: number
}

export interface ClozeDocument {
  indices: readonly string[]
  occurrences: readonly ClozeOccurrence[]
  source: string
  variantKeys: readonly string[]
}

export const ClozeProjection = {
  Answer: 'ANSWER',
  Question: 'QUESTION',
} as const

export type ClozeProjection =
  (typeof ClozeProjection)[keyof typeof ClozeProjection]

interface SourceRange {
  end: number
  start: number
}

export function parseClozeMarkdown(source: string): ClozeDocument {
  const excludedRanges = codeRanges(parseDaraMarkdownAst(source))
  const occurrences: ClozeOccurrence[] = []
  let offset = 0

  while (offset < source.length) {
    const excluded = containingOrNextRange(excludedRanges, offset)
    if (excluded?.start === offset) {
      offset = excluded.end
      continue
    }
    if (excluded && excluded.start < offset && offset < excluded.end) {
      offset = excluded.end
      continue
    }

    if (
      source.startsWith(CLOZE_OPEN, offset) &&
      !isEscaped(source, offset)
    ) {
      const occurrence = parseOccurrence(source, offset, excludedRanges)
      occurrences.push(occurrence)
      offset = occurrence.end
      continue
    }
    if (
      source.startsWith(CLOZE_CLOSE, offset) &&
      !isEscaped(source, offset)
    ) {
      throw new ClozeParseError(
        ClozeParseErrorCode.UnexpectedClose,
        'Found a closing “}}” without a matching cloze opening.',
        offset,
      )
    }
    offset += 1
  }

  if (occurrences.length === 0) {
    throw new ClozeParseError(
      ClozeParseErrorCode.MissingCloze,
      'Add at least one cloze such as {{c1::answer}}.',
    )
  }

  const indices = [...new Set(occurrences.map((occurrence) => occurrence.index))]
    .sort(compareDecimalIndices)
  return {
    indices,
    occurrences,
    source,
    variantKeys: indices.map(clozeVariantKey),
  }
}

export function projectClozeMarkdown(
  document: ClozeDocument,
  projection: ClozeProjection,
  selectedIndex?: string,
  renderQuestionPlaceholder: ClozeQuestionPlaceholderRenderer =
    defaultQuestionPlaceholder,
): string {
  if (
    projection === ClozeProjection.Question &&
    (!selectedIndex || !document.indices.includes(selectedIndex))
  ) {
    throw new ClozeParseError(
      ClozeParseErrorCode.UnknownVariant,
      `Cloze c${selectedIndex ?? ''} is not present in this card.`,
    )
  }

  let result = ''
  let offset = 0
  for (const occurrence of document.occurrences) {
    result += document.source.slice(offset, occurrence.start)
    if (
      projection === ClozeProjection.Question &&
      occurrence.index === selectedIndex
    ) {
      const placeholder =
        occurrence.hintMarkdown === null
          ? DEFAULT_QUESTION_PLACEHOLDER
          : unescapeClozeDelimiters(occurrence.hintMarkdown)
      result += renderQuestionPlaceholder(placeholder)
    } else {
      result += unescapeClozeDelimiters(occurrence.answerMarkdown)
    }
    offset = occurrence.end
  }
  return result + document.source.slice(offset)
}

function defaultQuestionPlaceholder(placeholderMarkdown: string): string {
  return `\\[${placeholderMarkdown}\\]`
}

export function clozeQuestionMarkdown(
  source: string,
  variantKey: string,
): string {
  const document = parseClozeMarkdown(source)
  return projectClozeMarkdown(
    document,
    ClozeProjection.Question,
    clozeIndexFromVariantKey(variantKey),
  )
}

export function clozeAnswerMarkdown(source: string): string {
  return projectClozeMarkdown(
    parseClozeMarkdown(source),
    ClozeProjection.Answer,
  )
}

export function clozeVariantKey(index: string): string {
  return `${CLOZE_VARIANT_PREFIX}${index}`
}

export function clozeIndexFromVariantKey(variantKey: string): string {
  const index = variantKey.startsWith(CLOZE_VARIANT_PREFIX)
    ? variantKey.slice(CLOZE_VARIANT_PREFIX.length)
    : ''
  if (!isCanonicalPositiveIndex(index)) {
    throw new ClozeParseError(
      ClozeParseErrorCode.UnknownVariant,
      `Invalid cloze variant key “${variantKey}”.`,
    )
  }
  return index
}

function parseOccurrence(
  source: string,
  start: number,
  excludedRanges: readonly SourceRange[],
): ClozeOccurrence {
  let offset = start + CLOZE_OPEN.length
  if (source[offset] !== 'c') {
    throw new ClozeParseError(
      ClozeParseErrorCode.InvalidHeader,
      'A cloze must start with “{{c” followed by a positive number.',
      start,
    )
  }
  offset += 1
  const indexStart = offset
  while (isAsciiDigit(source[offset])) {
    offset += 1
  }
  const index = source.slice(indexStart, offset)
  if (!isCanonicalPositiveIndex(index)) {
    throw new ClozeParseError(
      ClozeParseErrorCode.InvalidIndex,
      'Cloze numbers must be positive integers without leading zeroes.',
      indexStart,
    )
  }
  if (!source.startsWith(CLOZE_FIELD_SEPARATOR, offset)) {
    throw new ClozeParseError(
      ClozeParseErrorCode.InvalidHeader,
      `Cloze c${index} must separate its number and answer with “::”.`,
      offset,
    )
  }

  offset += CLOZE_FIELD_SEPARATOR.length
  const answerStart = offset
  let hintSeparator: number | null = null
  let closeStart: number | null = null

  while (offset < source.length) {
    const excluded = containingOrNextRange(excludedRanges, offset)
    if (excluded?.start === offset) {
      offset = excluded.end
      continue
    }
    if (excluded && excluded.start < offset && offset < excluded.end) {
      offset = excluded.end
      continue
    }

    if (source.startsWith(CLOZE_OPEN, offset) && !isEscaped(source, offset)) {
      throw new ClozeParseError(
        ClozeParseErrorCode.NestedCloze,
        'Nested clozes are not supported.',
        offset,
      )
    }
    if (
      source.startsWith(CLOZE_FIELD_SEPARATOR, offset) &&
      !isEscaped(source, offset)
    ) {
      if (hintSeparator !== null) {
        throw new ClozeParseError(
          ClozeParseErrorCode.TooManyFields,
          'Escape literal colons inside a cloze hint with “\\:”.',
          offset,
        )
      }
      hintSeparator = offset
      offset += CLOZE_FIELD_SEPARATOR.length
      continue
    }
    if (source.startsWith(CLOZE_CLOSE, offset) && !isEscaped(source, offset)) {
      closeStart = offset
      break
    }
    offset += 1
  }

  if (closeStart === null) {
    throw new ClozeParseError(
      ClozeParseErrorCode.UnclosedCloze,
      `Cloze c${index} is missing its closing “}}”.`,
      start,
    )
  }

  const answerEnd = hintSeparator ?? closeStart
  const answerMarkdown = source.slice(answerStart, answerEnd)
  if (unescapeClozeDelimiters(answerMarkdown).trim().length === 0) {
    throw new ClozeParseError(
      ClozeParseErrorCode.EmptyAnswer,
      `Cloze c${index} needs a non-empty answer.`,
      answerStart,
    )
  }

  return {
    answerMarkdown,
    end: closeStart + CLOZE_CLOSE.length,
    hintMarkdown:
      hintSeparator === null
        ? null
        : source.slice(hintSeparator + CLOZE_FIELD_SEPARATOR.length, closeStart),
    index,
    start,
  }
}

function codeRanges(tree: Root): SourceRange[] {
  const ranges: SourceRange[] = []
  collectCodeRanges(tree, ranges)
  return ranges.sort((left, right) => left.start - right.start)
}

function collectCodeRanges(node: Nodes, ranges: SourceRange[]) {
  if (
    (node.type === 'code' || node.type === 'inlineCode') &&
    node.position?.start.offset !== undefined &&
    node.position.end.offset !== undefined
  ) {
    ranges.push({
      end: node.position.end.offset,
      start: node.position.start.offset,
    })
    return
  }
  if ('children' in node) {
    for (const child of node.children) {
      collectCodeRanges(child, ranges)
    }
  }
}

function containingOrNextRange(
  ranges: readonly SourceRange[],
  offset: number,
): SourceRange | undefined {
  return ranges.find((range) => range.end > offset)
}

function isEscaped(source: string, offset: number): boolean {
  let slashes = 0
  for (let index = offset - 1; index >= 0 && source[index] === '\\'; index -= 1) {
    slashes += 1
  }
  return slashes % 2 === 1
}

function isAsciiDigit(character: string | undefined): boolean {
  return character !== undefined && character >= '0' && character <= '9'
}

function isCanonicalPositiveIndex(index: string): boolean {
  return (
    index.length > 0 &&
    index[0] !== '0' &&
    [...index].every(isAsciiDigit)
  )
}

function compareDecimalIndices(left: string, right: string): number {
  return left.length - right.length || left.localeCompare(right)
}

function unescapeClozeDelimiters(value: string): string {
  return value.replace(/\\([\\{}:])/g, '$1')
}
