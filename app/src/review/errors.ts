export const CommandErrorCode = {
  InvalidInput: 'invalidInput',
  NotFound: 'notFound',
  StaleReviewContext: 'staleReviewContext',
  StaleSchedulerReplay: 'staleSchedulerReplay',
  StaleCardContent: 'staleCardContent',
  IdempotencyConflict: 'idempotencyConflict',
  DatabaseUnavailable: 'databaseUnavailable',
  CorruptReviewData: 'corruptReviewData',
  UnsupportedSchedulerConfig: 'unsupportedSchedulerConfig',
  DatabaseError: 'databaseError',
} as const

export type CommandErrorCode =
  (typeof CommandErrorCode)[keyof typeof CommandErrorCode]

const commandErrorCodes = new Set<string>(Object.values(CommandErrorCode))

export function commandErrorCode(error: unknown): CommandErrorCode | null {
  if (typeof error !== 'object' || error === null || !('code' in error)) {
    return null
  }
  return typeof error.code === 'string' && commandErrorCodes.has(error.code)
    ? (error.code as CommandErrorCode)
    : null
}

export function errorMessage(error: unknown): string {
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message
  }
  return error instanceof Error ? error.message : String(error)
}
