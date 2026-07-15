import { emit, listen } from '@tauri-apps/api/event'
import { useEffect, useRef, type KeyboardEvent } from 'react'
import { native } from '../../lib/native.ts'
import {
  BasicCardForm,
  type BasicCardFormHandle,
} from '../shared/BasicCardForm.tsx'

export function QuickAddWindow() {
  const formRef = useRef<BasicCardFormHandle>(null)

  const focusFront = () => {
    formRef.current?.focusFront()
  }

  const handleEscapeCapture = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Escape' || event.nativeEvent.isComposing) {
      return
    }
    const target = event.target
    if (
      target instanceof Element &&
      (target.closest('.formula-dialog, .code-language-popover') ||
        target.closest(".code-language-trigger[aria-expanded='true']"))
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

    void listen('quick-add-shown', focusFront).then((unlisten) => {
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
        <BasicCardForm
          onCancel={() => native.dismissQuickAdd()}
          onSaved={async () => {
            await emit('card-created').catch((cause: unknown) => {
              console.error(
                'Could not notify the review window about the new card',
                cause,
              )
            })
            await native.dismissQuickAdd()
          }}
          ref={formRef}
          variant="quick"
        />
      </div>
    </main>
  )
}
