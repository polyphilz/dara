import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent,
  type ClipboardEvent,
} from 'react'
import { parseClozeMarkdown, projectClozeMarkdown, ClozeProjection } from '../../cloze/cloze.ts'
import { DaraInput } from '../../components/DaraInput.tsx'
import { DaraSelect } from '../../components/DaraSelect.tsx'
import {
  RichTextEditor,
  type RichTextEditorHandle,
} from '../../markdown/RichTextEditor.tsx'
import {
  OcclusionEditor,
  type OcclusionEditorHandle,
} from '../../occlusion/OcclusionEditor.tsx'
import {
  OcclusionImagePicker,
  type OcclusionImagePickerHandle,
} from '../../occlusion/OcclusionImagePicker.tsx'
import {
  CardContentType,
  createCardContent,
  OcclusionMode,
  updateCardContent,
  type CardContent,
  type CardContentDraft,
  type CardContentListItem,
  type OcclusionDefinition,
} from '../../review/index.ts'
import { errorMessage } from '../../review/errors.ts'
import { createUuidV7 } from '../../review/uuid-v7.ts'
import {
  ingestClipboardImage,
  renewMediaLease,
} from '../../media/gateway.ts'
import {
  CardFormVariant,
  type CardFormVariant as CardFormVariantType,
} from './card-form.ts'

const CARD_TYPE_OPTIONS = [
  { label: 'Basic', value: CardContentType.Basic },
  { label: 'Cloze', value: CardContentType.Cloze },
  { label: 'Image occlusion', value: CardContentType.Occlusion },
] as const

export interface CardFormHandle {
  cancel: () => void
  focusPrimary: () => void
}

interface CardFormProps {
  initialContent?: CardContent
  onCancel: () => void | Promise<void>
  onFileDialogOpenChange?: (open: boolean) => void
  onSaved: (item?: CardContentListItem) => void | Promise<void>
  variant: CardFormVariantType
}

const MEDIA_LEASE_RENEWAL_INTERVAL_MILLIS = 15 * 60 * 1_000

export const CardForm = forwardRef<CardFormHandle, CardFormProps>(
  function CardForm(
    {
      initialContent,
      onCancel,
      onFileDialogOpenChange,
      onSaved,
      variant,
    },
    ref,
  ) {
    const primaryRef = useRef<RichTextEditorHandle>(null)
    const secondaryRef = useRef<RichTextEditorHandle>(null)
    const occlusionEditorRef = useRef<OcclusionEditorHandle>(null)
    const occlusionPickerRef = useRef<OcclusionImagePickerHandle>(null)
    const [cardType, setCardType] = useState(
      initialContent?.type ?? CardContentType.Basic,
    )
    const [front, setFront] = useState(initialContent?.frontMd ?? '')
    const [back, setBack] = useState(initialContent?.backMd ?? '')
    const [source, setSource] = useState(initialContent?.source ?? '')
    const [occlusion, setOcclusion] = useState<OcclusionDefinition | null>(
      initialContent?.type === CardContentType.Occlusion
        ? initialContent.occlusion
        : null,
    )
    const [error, setError] = useState<string | null>(null)
    const [saving, setSaving] = useState(false)
    const [primaryMediaPending, setPrimaryMediaPending] = useState(false)
    const [secondaryMediaPending, setSecondaryMediaPending] = useState(false)
    const [occlusionMediaPending, setOcclusionMediaPending] = useState(false)
    const [mediaLeaseId, setMediaLeaseId] = useState(() => createUuidV7())
    const hasOcclusion = occlusion !== null

    const resetForm = useCallback(() => {
      setCardType(initialContent?.type ?? CardContentType.Basic)
      setFront(initialContent?.frontMd ?? '')
      setBack(initialContent?.backMd ?? '')
      setSource(initialContent?.source ?? '')
      setOcclusion(
        initialContent?.type === CardContentType.Occlusion
          ? initialContent.occlusion
          : null,
      )
      setError(null)
      setPrimaryMediaPending(false)
      setSecondaryMediaPending(false)
      setOcclusionMediaPending(false)
    }, [initialContent])

    const focusPrimary = useCallback(() => {
      requestAnimationFrame(() => {
        if (cardType === CardContentType.Occlusion) {
          if (hasOcclusion) {
            occlusionEditorRef.current?.focus()
          } else {
            occlusionPickerRef.current?.focus()
          }
        } else {
          primaryRef.current?.focus()
        }
      })
    }, [cardType, hasOcclusion])

    const discard = useCallback(() => {
      if (saving) {
        return
      }
      setError(null)
      setMediaLeaseId(createUuidV7())
      resetForm()
    }, [resetForm, saving])

    const cancel = useCallback(async () => {
      if (saving) {
        return
      }
      discard()
      try {
        await onCancel()
      } catch (cause) {
        setError(errorMessage(cause))
      }
    }, [discard, onCancel, saving])

    useImperativeHandle(
      ref,
      () => ({
        cancel: () => void cancel(),
        focusPrimary,
      }),
      [cancel, focusPrimary],
    )
    useEffect(focusPrimary, [focusPrimary])

    useEffect(() => {
      const renewIfActive = () => {
        if (!document.hasFocus()) {
          return
        }
        void renewMediaLease(mediaLeaseId).catch((cause: unknown) => {
          setError(errorMessage(cause))
        })
      }
      const interval = window.setInterval(
        renewIfActive,
        MEDIA_LEASE_RENEWAL_INTERVAL_MILLIS,
      )
      window.addEventListener('focus', renewIfActive)
      return () => {
        window.clearInterval(interval)
        window.removeEventListener('focus', renewIfActive)
      }
    }, [mediaLeaseId])

    useEffect(() => {
      resetForm()
    }, [resetForm])

    const save = async () => {
      if (saving) {
        return
      }
      if (
        primaryMediaPending ||
        secondaryMediaPending ||
        occlusionMediaPending
      ) {
        setError('Wait for the pasted image to finish processing before saving.')
        return
      }

      let content: CardContentDraft
      if (cardType === CardContentType.Basic) {
        if (!front.trim()) {
          setError('Add a question before saving.')
          primaryRef.current?.focus()
          return
        }
        if (!back.trim()) {
          setError('Add an answer before saving.')
          secondaryRef.current?.focus()
          return
        }
        content = {
          type: CardContentType.Basic,
          frontMd: front,
          backMd: back,
          source: source.trim() || null,
        }
      } else if (cardType === CardContentType.Cloze) {
        try {
          const document = parseClozeMarkdown(front)
          content = {
            type: CardContentType.Cloze,
            frontMd: front,
            backMd: back,
            source: source.trim() || null,
            searchMd: projectClozeMarkdown(
              document,
              ClozeProjection.Answer,
            ),
            variantKeys: [...document.variantKeys],
          }
        } catch (cause) {
          setError(errorMessage(cause))
          primaryRef.current?.focus()
          return
        }
      } else {
        if (!occlusion) {
          setError('Add an image before saving.')
          occlusionPickerRef.current?.focus()
          return
        }
        if (occlusion.layers.length === 0) {
          setError('Draw at least one mask layer before saving.')
          occlusionEditorRef.current?.focus()
          return
        }
        content = {
          type: CardContentType.Occlusion,
          frontMd: front,
          backMd: back,
          source: source.trim() || null,
          occlusion: {
            id: occlusion.id,
            sourceImageId: occlusion.sourceImage.id,
            mode: occlusion.mode,
            layers: occlusion.layers,
          },
        }
      }

      setError(null)
      setSaving(true)
      try {
        if (initialContent) {
          const item = await updateCardContent(
            {
              id: initialContent.id,
              expectedUpdatedAt: initialContent.updatedAt,
              content,
            },
            mediaLeaseId,
          )
          setMediaLeaseId(createUuidV7())
          await onSaved(item)
        } else {
          await createCardContent(content, mediaLeaseId)
          setMediaLeaseId(createUuidV7())
          setFront('')
          setBack('')
          setSource('')
          setOcclusion(null)
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

      if (
        event.key === 'Escape' &&
        variant === CardFormVariant.Quick
      ) {
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

    const handlePasteCapture = (event: ClipboardEvent<HTMLElement>) => {
      if (
        cardType !== CardContentType.Occlusion ||
        !Array.from(event.clipboardData.items).some((item) =>
          item.type.startsWith('image/'),
        )
      ) {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      setError(null)
      occlusionPickerRef.current?.ingestClipboard()
    }

    const editing = initialContent !== undefined
    const mediaPending =
      primaryMediaPending || secondaryMediaPending || occlusionMediaPending
    const quick = variant === CardFormVariant.Quick
    const primaryLabel =
      cardType === CardContentType.Basic
        ? 'Front'
        : cardType === CardContentType.Cloze
          ? 'Text'
          : 'Prompt'
    const secondaryLabel =
      cardType === CardContentType.Basic ? 'Back' : 'Extra'
    const typeLabel = cardType
    const formLabel = editing ? 'Edit card' : quick ? 'Quick add' : 'Add a card'

    return (
      <section
        className={`basic-card-form basic-card-form-${variant}`}
        aria-label={formLabel}
        onKeyDown={handleKeyDown}
        onPasteCapture={handlePasteCapture}
      >
        {editing ? (
          <header className="card-editor-header">
            <div>
              <p>{typeLabel} card</p>
              <h1>Edit card</h1>
            </div>
            <span>
              {quick ? 'Esc to cancel' : 'Rich text · Markdown saved automatically'}
            </span>
          </header>
        ) : (
          <div className="card-type-picker">
            <span>Card type</span>
            <DaraSelect
              ariaLabel="Card type"
              disabled={saving || mediaPending}
              menuHeight={112}
              menuWidth={160}
              onSelect={(nextCardType) => {
                setCardType(nextCardType)
                setError(null)
                focusPrimary()
              }}
              options={CARD_TYPE_OPTIONS}
              triggerClassName="card-type-trigger"
              value={cardType}
            />
          </div>
        )}

        {cardType === CardContentType.Occlusion && (
          <>
            <OcclusionImagePicker
              disabled={saving}
              leaseId={mediaLeaseId}
              onError={(cause) => setError(errorMessage(cause))}
              onFileDialogOpenChange={onFileDialogOpenChange}
              onImage={(image) => {
                setError(null)
                setOcclusion((current) =>
                  current
                    ? { ...current, sourceImage: image }
                    : {
                        id: createUuidV7(),
                        sourceImage: image,
                        mode: OcclusionMode.HideOneGuessOne,
                        layers: [],
                      },
                )
              }}
              onPendingChange={setOcclusionMediaPending}
              ref={occlusionPickerRef}
              showDropZone={!occlusion}
            />
            {occlusion && (
              <OcclusionEditor
                definition={occlusion}
                disabled={saving}
                key={`${occlusion.id}:${occlusion.sourceImage.id}`}
                onChange={setOcclusion}
                onReplaceImage={() => occlusionPickerRef.current?.open()}
                ref={occlusionEditorRef}
              />
            )}
          </>
        )}

        <div className={cardType === CardContentType.Occlusion ? 'occlusion-card-fields' : undefined}>
          <div className="card-editor-field">
            <span>
              {primaryLabel}{' '}
              {cardType === CardContentType.Occlusion && <small>optional</small>}
            </span>
            <RichTextEditor
              ariaLabel={primaryLabel}
              disabled={saving}
              ingestImage={() => ingestClipboardImage(mediaLeaseId)}
              key={`primary-${cardType}-${mediaLeaseId}`}
              onChange={setFront}
              onMediaError={(cause) => setError(errorMessage(cause))}
              onPendingMediaChange={(pending) => {
                setPrimaryMediaPending(pending)
                if (pending) {
                  setError(null)
                }
              }}
              placeholder={
                cardType === CardContentType.Basic
                  ? 'Question'
                  : cardType === CardContentType.Cloze
                    ? 'The capital of France is {{c1::Paris}}.'
                    : 'Optional context shown above the image'
              }
              ref={primaryRef}
              value={front}
            />
            {cardType === CardContentType.Cloze && (
              <small className="cloze-syntax-hint">
                Use {'{{c1::answer}}'} or {'{{c1::answer::hint}}'}
              </small>
            )}
          </div>

          <div className="card-editor-field card-editor-secondary-field">
            <span>
              {secondaryLabel}{' '}
              {cardType !== CardContentType.Basic && <small>optional</small>}
            </span>
            <RichTextEditor
              ariaLabel={secondaryLabel}
              disabled={saving}
              ingestImage={() => ingestClipboardImage(mediaLeaseId)}
              key={`secondary-${cardType}-${mediaLeaseId}`}
              onChange={setBack}
              onMediaError={(cause) => setError(errorMessage(cause))}
              onPendingMediaChange={(pending) => {
                setSecondaryMediaPending(pending)
                if (pending) {
                  setError(null)
                }
              }}
              placeholder={
                cardType === CardContentType.Basic
                  ? 'Answer'
                  : 'Supplemental explanation'
              }
              ref={secondaryRef}
              value={back}
            />
          </div>
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
            placeholder="URL, book title, lecture…"
            type="text"
            value={source}
          />
        </label>

        <footer className="card-editor-footer">
          <div>
            <button
              className="save-button"
              disabled={saving || mediaPending}
              onClick={() => void save()}
              type="button"
            >
              {mediaPending
                ? 'Processing image…'
                : saving
                  ? editing
                    ? 'Saving…'
                    : 'Adding…'
                  : editing
                    ? 'Save'
                    : 'Add'}{' '}
              <kbd>
                <span>⌘</span>
                <span className="save-shortcut-enter">↵</span>
              </kbd>
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
  },
)
