import { fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import { OcclusionImagePicker } from '../../../src/occlusion/OcclusionImagePicker.tsx'
import { OcclusionImageFileAccept } from '../../../src/occlusion/image-formats.ts'
import { ImageOcrStatus } from '../../../src/media/image-reference.ts'

const mocks = vi.hoisted(() => ({
  ingestClipboardImage: vi.fn(),
  ingestImageFile: vi.fn(),
}))

vi.mock('../../../src/media/gateway.ts', () => mocks)

const image = {
  id: '01980c8e-6c00-7000-8000-000000000301',
  mimeType: 'image/webp',
  naturalHeight: 400,
  naturalWidth: 800,
  ocrStatus: ImageOcrStatus.Pending,
}
const leaseId = '01980c8e-6c00-7000-8000-000000000399'

beforeEach(() => {
  vi.clearAllMocks()
  mocks.ingestClipboardImage.mockResolvedValue(image)
  mocks.ingestImageFile.mockResolvedValue(image)
})

test('accepts paste, file-picker, and drag-and-drop images through one restricted picker', async () => {
  const onImage = vi.fn()
  const { container, getByRole, queryByText } = render(
    <OcclusionImagePicker
      leaseId={leaseId}
      onError={vi.fn()}
      onImage={onImage}
      onPendingChange={vi.fn()}
      showDropZone
    />,
  )
  const picker = getByRole('button', { name: 'Choose an image for occlusion' })
  expect(queryByText('⌘V')).toBeNull()

  fireEvent.paste(picker, {
    clipboardData: { items: [{ type: 'image/png' }] },
  })
  await waitFor(() =>
    expect(mocks.ingestClipboardImage).toHaveBeenCalledWith(leaseId),
  )
  await waitFor(() => expect(onImage).toHaveBeenCalledTimes(1))

  const file = new File(['png'], 'diagram.png', { type: 'image/png' })
  const input = container.querySelector<HTMLInputElement>('input[type="file"]')!
  expect(input.accept).toBe(
    '.jpg,.jpeg,.jfif,.png,.apng,.webp,.tif,.tiff',
  )
  expect(input.accept).toBe(OcclusionImageFileAccept)
  fireEvent.change(input, { target: { files: [file] } })
  await waitFor(() =>
    expect(mocks.ingestImageFile).toHaveBeenCalledWith(file, leaseId),
  )
  await waitFor(() => expect(onImage).toHaveBeenCalledTimes(2))

  fireEvent.drop(picker, { dataTransfer: { files: [file] } })
  await waitFor(() => expect(mocks.ingestImageFile).toHaveBeenCalledTimes(2))
  await waitFor(() => expect(onImage).toHaveBeenCalledTimes(3))
})

test('rejects unsupported selections and drops before ingestion', () => {
  const onError = vi.fn()
  const { container, getByRole } = render(
    <OcclusionImagePicker
      leaseId={leaseId}
      onError={onError}
      onImage={vi.fn()}
      onPendingChange={vi.fn()}
      showDropZone
    />,
  )
  const markdown = new File(['# notes'], 'notes.md', {
    type: 'text/markdown',
  })
  const heic = new File(['heic'], 'photo.heic', { type: 'image/heic' })
  const input = container.querySelector<HTMLInputElement>('input[type="file"]')!

  fireEvent.change(input, { target: { files: [markdown] } })
  fireEvent.drop(
    getByRole('button', { name: 'Choose an image for occlusion' }),
    { dataTransfer: { files: [heic] } },
  )

  expect(onError).toHaveBeenCalledTimes(2)
  expect(onError.mock.calls[0]![0].message).toBe(
    'Choose a JPEG, PNG, WebP, or TIFF image.',
  )
  expect(mocks.ingestImageFile).not.toHaveBeenCalled()
})

test('ignores an ingestion result after the picker is abandoned', async () => {
  let resolveImage!: (record: typeof image) => void
  mocks.ingestClipboardImage.mockImplementation(
    () =>
      new Promise((resolve) => {
        resolveImage = resolve
      }),
  )
  const onImage = vi.fn()
  const { getByRole, unmount } = render(
    <OcclusionImagePicker
      leaseId={leaseId}
      onError={vi.fn()}
      onImage={onImage}
      onPendingChange={vi.fn()}
      showDropZone
    />,
  )

  fireEvent.paste(
    getByRole('button', { name: 'Choose an image for occlusion' }),
    { clipboardData: { items: [{ type: 'image/png' }] } },
  )
  await waitFor(() =>
    expect(mocks.ingestClipboardImage).toHaveBeenCalledWith(leaseId),
  )
  unmount()
  resolveImage(image)

  await Promise.resolve()
  expect(onImage).not.toHaveBeenCalled()
})
