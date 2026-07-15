import {
  useCallback,
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react'
import { DaraInput } from '../../components/DaraInput.tsx'
import {
  RichTextEditor,
  type RichTextEditorHandle,
} from '../../markdown/RichTextEditor.tsx'
import {
  CardContentType,
  createCardContent,
  updateCardContent,
  type BasicCardContent,
  type CardContentListItem,
} from '../../review/index.ts'
import {
  BasicCardFormVariant,
  type BasicCardFormVariant as BasicCardFormVariantType,
} from './card-form.ts'
import { errorMessage } from '../../review/errors.ts'

export interface BasicCardFormHandle {
  cancel: () => void
  focusFront: () => void
}

interface BasicCardFormProps {
  initialContent?: BasicCardContent
  onCancel: () => void | Promise<void>
  onSaved: (item?: CardContentListItem) => void | Promise<void>
  variant: BasicCardFormVariantType
}

export const BasicCardForm = forwardRef<
  BasicCardFormHandle,
  BasicCardFormProps
>(function BasicCardForm({ initialContent, onCancel, onSaved, variant }, ref) {
  const frontRef = useRef<RichTextEditorHandle>(null)
  const backRef = useRef<RichTextEditorHandle>(null)
  const [front, setFront] = useState(initialContent?.frontMd ?? '')
  const [back, setBack] = useState(initialContent?.backMd ?? '')
  const [source, setSource] = useState(initialContent?.source ?? '')
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  const focusFront = useCallback(() => {
    requestAnimationFrame(() => frontRef.current?.focus())
  }, [])

  const cancel = useCallback(async () => {
    if (saving) {
      return
    }
    setError(null)
    try {
      await onCancel()
    } catch (cause) {
      setError(errorMessage(cause))
    }
  }, [onCancel, saving])

  useImperativeHandle(
    ref,
    () => ({ cancel: () => void cancel(), focusFront }),
    [cancel, focusFront],
  )
  useEffect(focusFront, [focusFront])

  useEffect(() => {
    setFront(initialContent?.frontMd ?? '')
    setBack(initialContent?.backMd ?? '')
    setSource(initialContent?.source ?? '')
    setError(null)
  }, [initialContent])

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
      const content = {
        type: CardContentType.Basic,
        frontMd: front,
        backMd: back,
        source: source.trim() || null,
      }
      if (initialContent) {
        const item = await updateCardContent({
          id: initialContent.id,
          expectedUpdatedAt: initialContent.updatedAt,
          content,
        })
        await onSaved(item)
      } else {
        await createCardContent(content)
        setFront('')
        setBack('')
        setSource('')
        await onSaved()
      }
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setSaving(false)
    }
  }

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.defaultPrevented) {
      return
    }

    if (event.key === 'Escape') {
      if (event.nativeEvent.isComposing) {
        return
      }
      event.preventDefault()
      void cancel()
      return
    }

    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault()
      void save()
    }
  }

  const editing = initialContent !== undefined
  const title = editing
    ? 'Edit card'
    : variant === BasicCardFormVariant.Quick
      ? 'Quick add'
      : 'Add a card'
  return (
    <section
      className={`basic-card-form basic-card-form-${variant}`}
      aria-labelledby={`${variant}-card-editor-title`}
      onKeyDown={handleKeyDown}
    >
      <header className="card-editor-header">
        <div>
          <p>{editing ? 'BASIC card' : 'New BASIC card'}</p>
          <h1 id={`${variant}-card-editor-title`}>{title}</h1>
        </div>
        <span>
          {variant === BasicCardFormVariant.Quick
            ? 'Esc to cancel'
            : 'Rich text · Markdown saved automatically'}
        </span>
      </header>

      <div className="card-editor-field">
        <span>Front</span>
        <RichTextEditor
          ariaLabel="Front"
          disabled={saving}
          onChange={setFront}
          placeholder="Question"
          ref={frontRef}
          value={front}
        />
      </div>

      <div className="card-editor-field card-editor-secondary-field">
        <span>Back</span>
        <RichTextEditor
          ariaLabel="Back"
          disabled={saving}
          onChange={setBack}
          placeholder="Answer"
          ref={backRef}
          value={back}
        />
      </div>

      <label className="card-editor-field card-editor-secondary-field">
        <span>
          Source <small>optional</small>
        </span>
        <DaraInput
          disabled={saving}
          onChange={(event) => setSource(event.target.value)}
          onKeyDown={(event) => {
            if (
              event.key === 'Enter' &&
              !event.metaKey &&
              !event.ctrlKey &&
              !event.altKey
            ) {
              event.preventDefault()
            }
          }}
          placeholder="Book, article, lecture…"
          type="text"
          value={source}
        />
      </label>

      <footer className="card-editor-footer">
        <span className="editor-note">Both fields are required.</span>
        <div>
          <button
            className="save-button"
            disabled={saving}
            onClick={() => void save()}
            type="button"
          >
            {saving ? (editing ? 'Saving…' : 'Adding…') : editing ? 'Save' : 'Add'}{' '}
            <kbd>⌘↵</kbd>
          </button>
          <button
            className="cancel-button"
            disabled={saving}
            onClick={() => void cancel()}
            type="button"
          >
            Cancel
          </button>
        </div>
      </footer>

      {error && (
        <p className="card-editor-error" role="alert">
          {error}
        </p>
      )}
    </section>
  )
})
