export const ImageOcrStatus = {
  Pending: 'PENDING',
  Ready: 'READY',
  Failed: 'FAILED',
} as const

export type ImageOcrStatus =
  (typeof ImageOcrStatus)[keyof typeof ImageOcrStatus]

export interface ImageRecord {
  id: string
  mimeType: string
  naturalWidth: number
  naturalHeight: number
  ocrStatus: ImageOcrStatus
}

export interface ImageReference {
  imageId: string
  displayWidthPercent: number
}

export const DefaultImageDisplayWidthPercent = 100
export const ImageDisplayWidthStep = 5
export const MinImageDisplayWidthPercent = 10
export const MaxImageDisplayWidthPercent = 100

const IMAGE_TOKEN_PATTERN =
  /^\{\{image:([0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12});width=(\d{1,3})%\}\}$/
const LOCAL_MEDIA_ORIGIN = 'dara-media://localhost'

export function parseImageReferenceToken(
  value: string,
): ImageReference | null {
  const match = IMAGE_TOKEN_PATTERN.exec(value.trim())
  if (!match) {
    return null
  }
  const displayWidthPercent = Number(match[2])
  if (
    !Number.isInteger(displayWidthPercent) ||
    displayWidthPercent < MinImageDisplayWidthPercent ||
    displayWidthPercent > MaxImageDisplayWidthPercent
  ) {
    return null
  }
  return {
    imageId: match[1]!,
    displayWidthPercent,
  }
}

export function serializeImageReference(reference: ImageReference): string {
  return `{{image:${reference.imageId};width=${reference.displayWidthPercent}%}}`
}

export function localMediaUrl(imageId: string): string {
  return `${LOCAL_MEDIA_ORIGIN}/image/${imageId}`
}

export function clampImageDisplayWidth(value: number): number {
  return Math.min(
    MaxImageDisplayWidthPercent,
    Math.max(MinImageDisplayWidthPercent, Math.round(value)),
  )
}

export function initialImageDisplayWidth(
  naturalWidth: number,
  editorWidth: number,
): number {
  if (editorWidth <= 0) {
    return DefaultImageDisplayWidthPercent
  }
  return clampImageDisplayWidth(Math.floor((naturalWidth / editorWidth) * 100))
}
