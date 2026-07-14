export function commandErrorCode(error: unknown): string | null {
  if (typeof error !== 'object' || error === null || !('code' in error)) {
    return null
  }
  return typeof error.code === 'string' ? error.code : null
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
