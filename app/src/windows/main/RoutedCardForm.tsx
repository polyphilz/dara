import { useCallback, useRef, useState } from 'react'
import { useBlocker } from '@tanstack/react-router'
import { DaraButtonVariant } from '../../components/dara-button-types.ts'
import type {
  CardContent,
  CardContentListItem,
} from '../../review/index.ts'
import {
  CardForm,
  type CardFormHandle,
} from '../shared/CardForm.tsx'
import { CardFormVariant } from '../shared/card-form.ts'
import { ConfirmationDialog } from './ConfirmationDialog.tsx'
import { ConfirmationDialogInitialFocus } from './confirmation-dialog.ts'

interface RoutedCardFormProps {
  initialContent?: CardContent
  onCancel: () => void
  onSaved: (item?: CardContentListItem) => void | Promise<void>
}

export function RoutedCardForm({
  initialContent,
  onCancel,
  onSaved,
}: RoutedCardFormProps) {
  const formRef = useRef<CardFormHandle>(null)
  const [dirty, setDirty] = useState(false)
  const shouldBlock = useCallback(() => dirty, [dirty])
  const blocker = useBlocker({
    disabled: !dirty,
    enableBeforeUnload: dirty,
    shouldBlockFn: shouldBlock,
    withResolver: true,
  })

  return (
    <>
      <CardForm
        initialContent={initialContent}
        onCancel={onCancel}
        onDirtyChange={setDirty}
        onSaved={onSaved}
        ref={formRef}
        variant={CardFormVariant.Main}
      />
      {blocker.status === 'blocked' && (
        <ConfirmationDialog
          busy={false}
          cancelLabel="Keep editing"
          confirmLabel="Discard changes"
          confirmVariant={DaraButtonVariant.Danger}
          initialFocus={ConfirmationDialogInitialFocus.Cancel}
          onCancel={() => {
            blocker.reset()
            requestAnimationFrame(() => formRef.current?.focusPrimary())
          }}
          onConfirm={blocker.proceed}
          title="Discard unsaved changes?"
        >
          <p>Your draft has not been saved.</p>
        </ConfirmationDialog>
      )}
    </>
  )
}
