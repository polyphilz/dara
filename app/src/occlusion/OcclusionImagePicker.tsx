import {
  forwardRef,
  useImperativeHandle,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
  type ClipboardEvent,
} from 'react'
import { ingestClipboardImage, ingestImageFile } from '../media/gateway.ts'
import type { ImageRecord } from '../media/image-reference.ts'
import {
  isSupportedOcclusionImageFile,
  OcclusionImageFileAccept,
} from './image-formats.ts'

export interface OcclusionImagePickerHandle {
  focus: () => void
  ingestClipboard: () => void
  open: () => void
}

interface OcclusionImagePickerProps {
  disabled?: boolean
  onError: (cause: unknown) => void
  onImage: (image: ImageRecord) => void
  onPendingChange: (pending: boolean) => void
  showDropZone: boolean
}

const unsupportedImageMessage =
  'Choose a JPEG, PNG, WebP, or TIFF image.'

export const OcclusionImagePicker = forwardRef<
  OcclusionImagePickerHandle,
  OcclusionImagePickerProps
>(function OcclusionImagePicker(
  {
    disabled = false,
    onError,
    onImage,
    onPendingChange,
    showDropZone,
  },
  ref,
) {
  const inputRef = useRef<HTMLInputElement>(null)
  const dropZoneRef = useRef<HTMLButtonElement>(null)
  const [pending, setPending] = useState(false)
  const [dragging, setDragging] = useState(false)

  const open = () => {
    if (!disabled && !pending) {
      inputRef.current?.click()
    }
  }
  const focus = () => {
    requestAnimationFrame(() => dropZoneRef.current?.focus())
  }
  const process = async (operation: () => Promise<ImageRecord>) => {
    if (disabled || pending) {
      return
    }
    setPending(true)
    onPendingChange(true)
    try {
      onImage(await operation())
    } catch (cause) {
      onError(cause)
    } finally {
      setPending(false)
      onPendingChange(false)
    }
  }

  const ingestClipboard = () => {
    void process(ingestClipboardImage)
  }
  useImperativeHandle(
    ref,
    () => ({ focus, ingestClipboard, open }),
  )

  const processFile = (file: File | undefined) => {
    if (!file) {
      return
    }
    if (!isSupportedOcclusionImageFile(file)) {
      onError(new Error(unsupportedImageMessage))
      return
    }
    void process(() => ingestImageFile(file))
  }

  const handlePaste = (event: ClipboardEvent<HTMLElement>) => {
    if (
      !Array.from(event.clipboardData.items).some((item) =>
        item.type.startsWith('image/'),
      )
    ) {
      return
    }
    event.preventDefault()
    ingestClipboard()
  }

  const handleDrop = (event: DragEvent<HTMLElement>) => {
    event.preventDefault()
    setDragging(false)
    const files = Array.from(event.dataTransfer.files)
    const image = files.find(isSupportedOcclusionImageFile)
    if (!image && files.length > 0) {
      onError(new Error(unsupportedImageMessage))
      return
    }
    processFile(image)
  }

  const fileInput = (
    <input
      accept={OcclusionImageFileAccept}
      aria-hidden="true"
      className="occlusion-file-input"
      disabled={disabled || pending}
      onChange={(event: ChangeEvent<HTMLInputElement>) => {
        processFile(event.target.files?.[0])
        event.target.value = ''
      }}
      ref={inputRef}
      tabIndex={-1}
      type="file"
    />
  )

  if (!showDropZone) {
    return fileInput
  }

  return (
    <div onPaste={handlePaste}>
      {fileInput}
      <button
        aria-label="Choose an image for occlusion"
        className={`occlusion-image-picker${dragging ? ' dragging' : ''}`}
        disabled={disabled || pending}
        onClick={open}
        onDragEnter={(event) => {
          event.preventDefault()
          setDragging(true)
        }}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
            setDragging(false)
          }
        }}
        onDragOver={(event) => event.preventDefault()}
        onDrop={handleDrop}
        ref={dropZoneRef}
        type="button"
      >
        <span className="occlusion-picker-icon" aria-hidden="true">▧</span>
        <strong>{pending ? 'Processing image…' : 'Add an image'}</strong>
        <span>{pending ? 'Re-encoding and saving locally' : 'Paste, drag and drop, or click to choose'}</span>
        {!pending && <kbd>⌘V</kbd>}
      </button>
    </div>
  )
})
