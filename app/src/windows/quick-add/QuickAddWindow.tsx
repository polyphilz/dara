import { emit, listen } from '@tauri-apps/api/event'
import { useEffect, useRef, type KeyboardEvent } from 'react'
import { native } from '../../lib/native.ts'
import { DaraEvent } from '../../lib/tauri-contracts.ts'
import { CardForm, type CardFormHandle } from '../shared/CardForm.tsx'
import { CardFormVariant } from '../shared/card-form.ts'

function setFileDialogOpen(open: boolean) {
  void native.setQuickAddFileDialogOpen(open).catch((cause: unknown) => {
    console.error('Could not update Quick Add file-dialog state', cause)
  })
}

export function QuickAddWindow() {
  const formRef = useRef<CardFormHandle>(null)

  const focusFront = () => {
    formRef.current?.focusPrimary()
  }

  const handleEscapeCapture = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Escape' || event.nativeEvent.isComposing) {
      return
    }
    const target = event.target
    if (
      target instanceof Element &&
      (target.closest(
        '.formula-dialog, .dara-select-popover, .occlusion-layer-panel, .occlusion-editor-legend-open',
      ) ||
        target.closest(".dara-select-trigger[aria-expanded='true']"))
    ) {
      return
    }

    event.preventDefault()
    event.stopPropagation()
    formRef.current?.cancel()
  }

  useEffect(() => {
    focusFront()
    let disposed = false
    let stopListening: (() => void) | undefined

    void listen(DaraEvent.QuickAddShown, focusFront).then((unlisten) => {
      if (disposed) {
        unlisten()
      } else {
        stopListening = unlisten
      }
    })

    return () => {
      disposed = true
      stopListening?.()
    }
  }, [])

  return (
    <main className="quick-add-shell" onKeyDownCapture={handleEscapeCapture}>
      <div className="quick-add-card">
        <h1 className="visually-hidden">Quick add</h1>
        <CardForm
          onCancel={() => native.dismissQuickAdd()}
          onFileDialogOpenChange={setFileDialogOpen}
          onSaved={async () => {
            await emit(DaraEvent.CardCreated).catch((cause: unknown) => {
              console.error(
                'Could not notify the review window about the new card',
                cause,
              )
            })
            await native.dismissQuickAdd()
          }}
          ref={formRef}
          variant={CardFormVariant.Quick}
        />
      </div>
    </main>
  )
}
