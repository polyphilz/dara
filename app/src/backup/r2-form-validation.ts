export interface R2ConnectionFormValues {
  accountId: string
  bucket: string
  accessKeyId: string
  secretAccessKey: string
}

export interface R2ConnectionFormErrors {
  accountId?: string
  bucket?: string
  accessKeyId?: string
  secretAccessKey?: string
}

export function validateR2ConnectionForm(
  form: R2ConnectionFormValues,
  includeTarget = true,
): R2ConnectionFormErrors {
  const errors: R2ConnectionFormErrors = {}
  const lowerHex = /^[0-9a-f]+$/
  if (includeTarget) {
    if (form.accountId.length !== 32 || !lowerHex.test(form.accountId)) {
      errors.accountId = 'Enter the 32-character lowercase R2 account ID.'
    }
    if (!/^[a-z0-9](?:[a-z0-9-]{1,61}[a-z0-9])$/.test(form.bucket)) {
      errors.bucket = 'Use 3–63 lowercase letters, numbers, or hyphens.'
    }
  }
  if (form.accessKeyId.length !== 32 || !lowerHex.test(form.accessKeyId)) {
    errors.accessKeyId =
      'Enter the 32-character lowercase R2 Access Key ID.'
  }
  if (
    form.secretAccessKey.length !== 64 ||
    !lowerHex.test(form.secretAccessKey)
  ) {
    errors.secretAccessKey =
      'Enter the 64-character lowercase R2 Secret Access Key.'
  }
  return errors
}
