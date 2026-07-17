import {
  forwardRef,
  useCallback,
  useEffect,
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
  leaseId: string
  onError: (cause: unknown) => void
  onFileDialogOpenChange?: (open: boolean) => void
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
    leaseId,
    onError,
    onFileDialogOpenChange,
    onImage,
    onPendingChange,
    showDropZone,
  },
  ref,
) {
  const inputRef = useRef<HTMLInputElement>(null)
  const dropZoneRef = useRef<HTMLButtonElement>(null)
  const activeRef = useRef(true)
  const fileDialogOpenRef = useRef(false)
  const leaseIdRef = useRef(leaseId)
  const [pending, setPending] = useState(false)
  const [dragging, setDragging] = useState(false)
  leaseIdRef.current = leaseId

  useEffect(() => {
    activeRef.current = true
    return () => {
      activeRef.current = false
      if (fileDialogOpenRef.current) {
        onFileDialogOpenChange?.(false)
      }
    }
  }, [onFileDialogOpenChange])

  const setFileDialogOpen = useCallback(
    (open: boolean) => {
      if (fileDialogOpenRef.current === open) {
        return
      }
      fileDialogOpenRef.current = open
      onFileDialogOpenChange?.(open)
    },
    [onFileDialogOpenChange],
  )

  useEffect(() => {
    const closeOnFocusReturn = () => setFileDialogOpen(false)
    window.addEventListener('focus', closeOnFocusReturn)
    return () => window.removeEventListener('focus', closeOnFocusReturn)
  }, [setFileDialogOpen])

  useEffect(() => {
    const input = inputRef.current
    if (!input) {
      return
    }
    const closeOnCancel = () => setFileDialogOpen(false)
    input.addEventListener('cancel', closeOnCancel)
    return () => input.removeEventListener('cancel', closeOnCancel)
  }, [setFileDialogOpen])

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
    const requestLeaseId = leaseId
    setPending(true)
    onPendingChange(true)
    try {
      const image = await operation()
      if (activeRef.current && leaseIdRef.current === requestLeaseId) {
        onImage(image)
      }
    } catch (cause) {
      if (activeRef.current && leaseIdRef.current === requestLeaseId) {
        onError(cause)
      }
    } finally {
      if (activeRef.current && leaseIdRef.current === requestLeaseId) {
        setPending(false)
        onPendingChange(false)
      }
    }
  }

  const ingestClipboard = () => {
    void process(() => ingestClipboardImage(leaseId))
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
    void process(() => ingestImageFile(file, leaseId))
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
        setFileDialogOpen(false)
        processFile(event.target.files?.[0])
        event.target.value = ''
      }}
      onClick={() => setFileDialogOpen(true)}
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
        <span className="occlusion-picker-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24">
            <rect height="16" rx="2" width="18" x="3" y="4" />
            <circle cx="8.5" cy="9" r="1.5" />
            <path d="m4 18 4.5-4.5 3.5 3.5 2.5-2.5 5.5 5.5" />
          </svg>
        </span>
        <strong>{pending ? 'Processing image…' : 'Add an image'}</strong>
        <span>{pending ? 'Re-encoding and saving locally' : 'Paste, drag and drop, or click to choose'}</span>
      </button>
    </div>
  )
})
