import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  type ChangeEvent,
} from 'react'

export interface DaraFilePickerHandle {
  open: () => void
}

interface DaraFilePickerProps {
  accept?: string
  className?: string
  disabled?: boolean
  onFile: (file: File) => void
  onFileDialogOpenChange?: (open: boolean) => void
}

/** Shared native-file-dialog bridge for app-owned picker controls. */
export const DaraFilePicker = forwardRef<
  DaraFilePickerHandle,
  DaraFilePickerProps
>(function DaraFilePicker(
  {
    accept,
    className,
    disabled = false,
    onFile,
    onFileDialogOpenChange,
  },
  ref,
) {
  const inputRef = useRef<HTMLInputElement>(null)
  const fileDialogOpenRef = useRef(false)
  const onFileDialogOpenChangeRef = useRef(onFileDialogOpenChange)
  onFileDialogOpenChangeRef.current = onFileDialogOpenChange

  const setFileDialogOpen = useCallback((open: boolean) => {
    if (fileDialogOpenRef.current === open) {
      return
    }
    fileDialogOpenRef.current = open
    onFileDialogOpenChangeRef.current?.(open)
  }, [])

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

  useEffect(
    () => () => {
      setFileDialogOpen(false)
    },
    [setFileDialogOpen],
  )

  useImperativeHandle(
    ref,
    () => ({
      open: () => {
        if (!disabled) {
          inputRef.current?.click()
        }
      },
    }),
    [disabled],
  )

  return (
    <input
      accept={accept}
      aria-hidden="true"
      className={['visually-hidden', className].filter(Boolean).join(' ')}
      disabled={disabled}
      onChange={(event: ChangeEvent<HTMLInputElement>) => {
        setFileDialogOpen(false)
        const file = event.target.files?.[0]
        if (file) {
          onFile(file)
        }
        event.target.value = ''
      }}
      onClick={() => setFileDialogOpen(true)}
      ref={inputRef}
      tabIndex={-1}
      type="file"
    />
  )
})
