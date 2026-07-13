import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { listen } from '@tauri-apps/api/event'
import { native } from '../../lib/native.ts'

export function QuickAddWindow() {
  const frontRef = useRef<HTMLTextAreaElement>(null)
  const [front, setFront] = useState('')
  const [back, setBack] = useState('')
  const [error, setError] = useState<string | null>(null)

  const focusFront = () => {
    requestAnimationFrame(() => frontRef.current?.focus())
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

  const dismiss = () => {
    setError(null)
    void native.dismissQuickAdd().catch((cause: unknown) => setError(String(cause)))
  }

  const save = async () => {
    if (!front.trim()) {
      frontRef.current?.focus()
      return
    }

    setError(null)
    try {
      await native.saveSpikeCard(front, back)
      setFront('')
      setBack('')
    } catch (cause) {
      setError(String(cause))
    }
  }

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      dismiss()
      return
    }

    if (event.key === 'Enter' && event.metaKey) {
      event.preventDefault()
      void save()
    }
  }

  return (
    <main className="quick-add-shell" onKeyDown={handleKeyDown}>
      <section className="quick-add-card" aria-labelledby="quick-add-title">
        <header className="quick-add-header">
          <div>
            <p>Capture</p>
            <h1 id="quick-add-title">Quick add</h1>
          </div>
          <span>Esc to cancel</span>
        </header>

        <label className="field">
          <span>Front</span>
          <textarea
            ref={frontRef}
            rows={3}
            value={front}
            onChange={(event) => setFront(event.target.value)}
            placeholder="What do you want to remember?"
            spellCheck
          />
        </label>

        <label className="field secondary-field">
          <span>Back</span>
          <textarea
            rows={2}
            value={back}
            onChange={(event) => setBack(event.target.value)}
            placeholder="Answer or explanation"
            spellCheck
          />
        </label>

        <footer className="quick-add-footer">
          <span className="test-note">Spike only—nothing is persisted yet.</span>
          <div>
            <button className="cancel-button" type="button" onClick={dismiss}>
              Cancel
            </button>
            <button className="save-button" type="button" onClick={() => void save()}>
              Save <kbd>⌘↵</kbd>
            </button>
          </div>
        </footer>

        {error && (
          <p className="panel-error" role="alert">
            {error}
          </p>
        )}
      </section>
    </main>
  )
}
