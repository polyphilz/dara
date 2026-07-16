import { describe, expect, test } from 'vitest'
import {
  initialImageDisplayWidth,
  localMediaUrl,
  parseImageReferenceToken,
  serializeImageReference,
} from '../../../src/media/image-reference.ts'

const IMAGE_ID = '01980c8e-6c00-7000-8000-000000000201'

describe('Dara image references', () => {
  test('round-trips the canonical block token', () => {
    const token = `{{image:${IMAGE_ID};width=65%}}`
    const reference = parseImageReferenceToken(token)

    expect(reference).toEqual({
      imageId: IMAGE_ID,
      displayWidthPercent: 65,
    })
    expect(serializeImageReference(reference!)).toBe(token)
  })

  test.each([
    `{{image:${IMAGE_ID}}}`,
    `{{image:${IMAGE_ID};width=0%}}`,
    `{{image:${IMAGE_ID};width=101%}}`,
    `prefix {{image:${IMAGE_ID};width=50%}}`,
    '{{image:not-a-uuid;width=50%}}',
  ])('rejects noncanonical reference %s', (value) => {
    expect(parseImageReferenceToken(value)).toBeNull()
  })

  test('builds an ID-bound local-media URL', () => {
    expect(localMediaUrl(IMAGE_ID)).toBe(
      `dara-media://localhost/image/${IMAGE_ID}`,
    )
  })

  test('uses natural size for small images and fits larger images', () => {
    expect(initialImageDisplayWidth(320, 800)).toBe(40)
    expect(initialImageDisplayWidth(1_200, 800)).toBe(100)
    expect(initialImageDisplayWidth(320, 0)).toBe(100)
  })
})
