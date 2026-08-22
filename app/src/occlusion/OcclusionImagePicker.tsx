import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type DragEvent,
  type ClipboardEvent,
} from 'react'
import { ingestClipboardImage, ingestImageFile } from '../media/gateway.ts'
import { DaraButton } from '../components/DaraButton.tsx'
import {
  DaraFilePicker,
  type DaraFilePickerHandle,
} from '../components/DaraFilePicker.tsx'
import { DaraImageIcon } from '../components/DaraImageIcon.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../components/dara-button-types.ts'
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
  const filePickerRef = useRef<DaraFilePickerHandle>(null)
  const dropZoneRef = useRef<HTMLButtonElement>(null)
  const activeRef = useRef(true)
  const leaseIdRef = useRef(leaseId)
  const [pending, setPending] = useState(false)
  const [dragging, setDragging] = useState(false)
  leaseIdRef.current = leaseId

  useEffect(() => {
    activeRef.current = true
    return () => {
      activeRef.current = false
    }
  }, [])

  const open = () => {
    if (!disabled && !pending) {
      filePickerRef.current?.open()
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
    <DaraFilePicker
      accept={OcclusionImageFileAccept}
      className="occlusion-file-input"
      disabled={disabled || pending}
      onFile={processFile}
      onFileDialogOpenChange={onFileDialogOpenChange}
      ref={filePickerRef}
    />
  )

  if (!showDropZone) {
    return fileInput
  }

  return (
    <div onPaste={handlePaste}>
      {fileInput}
      <DaraButton
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
        size={DaraButtonSize.Custom}
        type="button"
        variant={DaraButtonVariant.Custom}
      >
        <span className="occlusion-picker-icon" aria-hidden="true">
          <DaraImageIcon />
        </span>
        <strong>{pending ? 'Processing image…' : 'Add an image'}</strong>
        <span>{pending ? 'Re-encoding and saving locally' : 'Paste, drag and drop, or click to choose'}</span>
      </DaraButton>
    </div>
  )
})
