import { useEffect, useRef, type ReactNode } from 'react'
import { DaraButton } from '../../components/DaraButton.tsx'
import {
  DaraButtonVariant,
  type DaraButtonVariant as DaraButtonVariantType,
} from '../../components/dara-button-types.ts'
import {
  ConfirmationDialogInitialFocus,
  type ConfirmationDialogInitialFocus as ConfirmationDialogInitialFocusType,
} from './confirmation-dialog.ts'

interface ConfirmationDialogProps {
  allowCancelWhileBusy?: boolean
  busy: boolean
  cancelLabel?: string
  children: ReactNode
  confirmLabel: string
  confirmVariant?: DaraButtonVariantType
  initialFocus?: ConfirmationDialogInitialFocusType
  onCancel: () => void
  onConfirm: () => void
  title: string
}

export function ConfirmationDialog({
  allowCancelWhileBusy = false,
  busy,
  cancelLabel = 'Cancel',
  children,
  confirmLabel,
  confirmVariant = DaraButtonVariant.Primary,
  initialFocus = ConfirmationDialogInitialFocus.Confirm,
  onCancel,
  onConfirm,
  title,
}: ConfirmationDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null)
  const confirmRef = useRef<HTMLButtonElement>(null)
  const formRef = useRef<HTMLFormElement>(null)

  useEffect(() => {
    const target =
      initialFocus === ConfirmationDialogInitialFocus.Cancel
        ? cancelRef.current
        : confirmRef.current
    target?.focus()
  }, [initialFocus])

  return (
    <div className="settings-dialog-backdrop">
      <form
        aria-label={title}
        className="settings-dialog"
        onKeyDown={(event) => {
          event.stopPropagation()
          if (event.key === 'Escape') {
            event.preventDefault()
            onCancel()
            return
          }
          if (event.key === 'Tab') {
            const focusable = Array.from(
              formRef.current?.querySelectorAll<HTMLButtonElement>(
                'button:not(:disabled)',
              ) ?? [],
            )
            if (focusable.length === 0) {
              event.preventDefault()
            } else if (
              event.shiftKey &&
              document.activeElement === focusable[0]
            ) {
              event.preventDefault()
              focusable.at(-1)?.focus()
            } else if (
              !event.shiftKey &&
              document.activeElement === focusable.at(-1)
            ) {
              event.preventDefault()
              focusable[0]?.focus()
            }
          }
        }}
        onSubmit={(event) => {
          event.preventDefault()
          if (!busy) {
            onConfirm()
          }
        }}
        role="alertdialog"
        ref={formRef}
      >
        <div>
          <span>Confirm change</span>
          <h2>{title}</h2>
        </div>
        <div className="settings-dialog-copy">{children}</div>
        <div className="settings-dialog-actions">
          <DaraButton
            disabled={busy && !allowCancelWhileBusy}
            onClick={onCancel}
            ref={cancelRef}
            type="button"
            variant={DaraButtonVariant.Ghost}
          >
            {cancelLabel}
          </DaraButton>
          <DaraButton
            disabled={busy}
            ref={confirmRef}
            type="submit"
            variant={confirmVariant}
          >
            {confirmLabel}
          </DaraButton>
        </div>
      </form>
    </div>
  )
}
