export interface TypographyViolation {
  path: string
  line: number
  check: string
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
