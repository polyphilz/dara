export const SupportedOcclusionImageExtension = {
  Jpg: '.jpg',
  Jpeg: '.jpeg',
  Jfif: '.jfif',
  Png: '.png',
  Apng: '.apng',
  WebP: '.webp',
  Tif: '.tif',
  Tiff: '.tiff',
} as const

export const SupportedOcclusionImageMimeType = {
  Jpeg: 'image/jpeg',
  Png: 'image/png',
  WebP: 'image/webp',
  Tiff: 'image/tiff',
} as const

export const OcclusionImageFileAccept = Object.values(
  SupportedOcclusionImageExtension,
).join(',')

const supportedExtensions = new Set<string>(
  Object.values(SupportedOcclusionImageExtension),
)
const supportedMimeTypes = new Set<string>(
  Object.values(SupportedOcclusionImageMimeType),
)

export function isSupportedOcclusionImageFile(file: File): boolean {
  const extensionIndex = file.name.lastIndexOf('.')
  const extension =
    extensionIndex >= 0 ? file.name.slice(extensionIndex).toLowerCase() : ''
  return supportedExtensions.has(extension) || supportedMimeTypes.has(file.type)
}
