export const TypographyCheckKind: {
  readonly FontSize: 'font-size'
  readonly FontShorthand: 'font shorthand'
  readonly InlineFontSize: 'fontSize'
  readonly FontWeight: 'font-weight'
  readonly LetterSpacing: 'letter-spacing'
  readonly LineHeight: 'line-height'
  readonly TypographyVariable: 'typography variable'
}

export type TypographyCheckKind =
  (typeof TypographyCheckKind)[keyof typeof TypographyCheckKind]

export interface TypographyViolation {
  path: string
  line: number
  check: TypographyCheckKind
  message: string
  source: string
}

/**
 * Returns every typography contract violation in one source file. `path` is
 * used for reporting; `relativePath` is matched against the exception list.
 */
export function findTypographyViolations(
  path: string,
  contents: string,
  relativePath: string,
): TypographyViolation[]
