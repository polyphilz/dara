import { emit, listen } from '@tauri-apps/api/event'
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { native } from '../../lib/native.ts'
import { createBasicCard } from '../../review/index.ts'
import { errorMessage } from '../../review/errors.ts'

export function QuickAddWindow() {
  const frontRef = useRef<HTMLTextAreaElement>(null)
  const backRef = useRef<HTMLTextAreaElement>(null)
  const [front, setFront] = useState('')
  const [back, setBack] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

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

  const dismiss = async () => {
    if (saving) {
      return
    }
    setError(null)
    try {
      await native.dismissQuickAdd()
    } catch (cause) {
      setError(errorMessage(cause))
    }
  }

  const save = async () => {
    if (saving) {
      return
    }
    if (!front.trim()) {
      setError('Add a question before saving.')
      frontRef.current?.focus()
      return
    }
    if (!back.trim()) {
      setError('Add an answer before saving.')
      backRef.current?.focus()
      return
    }

    setError(null)
    setSaving(true)
    try {
      await createBasicCard({
        frontMd: front.trim(),
        backMd: back.trim(),
        source: null,
      })
      setFront('')
      setBack('')
      await emit('card-created').catch((cause: unknown) => {
        console.error('Could not notify the review window about the new card', cause)
      })
      await native.dismissQuickAdd()
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setSaving(false)
    }
  }

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      void dismiss()
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
            <p>New BASIC card</p>
            <h1 id="quick-add-title">Quick add</h1>
          </div>
          <span>Esc to cancel</span>
        </header>

        <label className="field">
          <span>Front</span>
          <textarea
            disabled={saving}
            ref={frontRef}
            rows={3}
            value={front}
            onChange={(event) => setFront(event.target.value)}
            placeholder="Question"
            spellCheck
          />
        </label>

        <label className="field secondary-field">
          <span>Back</span>
          <textarea
            disabled={saving}
            ref={backRef}
            rows={2}
            value={back}
            onChange={(event) => setBack(event.target.value)}
            placeholder="Answer"
            spellCheck
          />
        </label>

        <footer className="quick-add-footer">
          <span className="test-note">Both fields are required.</span>
          <div>
            <button
              className="cancel-button"
              disabled={saving}
              type="button"
              onClick={() => void dismiss()}
            >
              Cancel
            </button>
            <button
              className="save-button"
              disabled={saving}
              type="button"
              onClick={() => void save()}
            >
              {saving ? 'Saving…' : 'Save'} <kbd>⌘↵</kbd>
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
