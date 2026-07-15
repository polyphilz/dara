import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react'
import { DaraInput } from '../../components/DaraInput.tsx'
import { CardSource } from '../../markdown/CardSource.tsx'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer.tsx'
import {
  deleteCardContent,
  searchCardContent,
  setCardContentSuspended,
  type CardContentListItem,
} from '../../review/index.ts'
import { errorMessage } from '../../review/errors.ts'
import { BasicCardForm } from '../shared/BasicCardForm.tsx'

const SEARCH_LIMIT = 75

interface CardBrowserProps {
  onQueueChanged: () => void
  refreshToken?: number
}

export function CardBrowser({ onQueueChanged, refreshToken = 0 }: CardBrowserProps) {
  const searchRef = useRef<HTMLInputElement>(null)
  const requestId = useRef(0)
  const [query, setQuery] = useState('')
  const [revision, setRevision] = useState(0)
  const [results, setResults] = useState<CardContentListItem[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  const [confirmingDelete, setConfirmingDelete] = useState(false)
  const [loading, setLoading] = useState(true)
  const [mutating, setMutating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const selected = useMemo(
    () => results.find((item) => item.cardContent.id === selectedId) ?? null,
    [results, selectedId],
  )

  useEffect(() => {
    searchRef.current?.focus()
  }, [])

  useEffect(() => {
    const currentRequest = ++requestId.current
    setLoading(true)
    setError(null)
    void searchCardContent({ query, limit: SEARCH_LIMIT })
      .then((nextResults) => {
        if (requestId.current !== currentRequest) {
          return
        }
        setResults(nextResults)
        setSelectedId((current) =>
          current && nextResults.some((item) => item.cardContent.id === current)
            ? current
            : (nextResults[0]?.cardContent.id ?? null),
        )
      })
      .catch((cause: unknown) => {
        if (requestId.current === currentRequest) {
          setError(errorMessage(cause))
        }
      })
      .finally(() => {
        if (requestId.current === currentRequest) {
          setLoading(false)
        }
      })
  }, [query, refreshToken, revision])

  const refresh = useCallback(() => {
    setRevision((value) => value + 1)
  }, [])

  const moveSelection = (delta: -1 | 1) => {
    if (results.length === 0) {
      return
    }
    const currentIndex = Math.max(
      0,
      results.findIndex((item) => item.cardContent.id === selectedId),
    )
    const nextIndex = Math.min(results.length - 1, Math.max(0, currentIndex + delta))
    setSelectedId(results[nextIndex]!.cardContent.id)
    setConfirmingDelete(false)
  }

  const toggleSuspended = async () => {
    if (!selected || mutating) {
      return
    }
    setMutating(true)
    setError(null)
    try {
      const item = await setCardContentSuspended({
        cardContentId: selected.cardContent.id,
        expectedLifecycleUpdatedAt: selected.lifecycleUpdatedAt,
        suspended: selected.reviewStatus !== 'SUSPENDED',
      })
      setResults((current) =>
        current.map((candidate) =>
          candidate.cardContent.id === item.cardContent.id ? item : candidate,
        ),
      )
      onQueueChanged()
    } catch (cause) {
      setError(errorMessage(cause))
      refresh()
    } finally {
      setMutating(false)
    }
  }

  const removeSelected = async () => {
    if (!selected || mutating) {
      return
    }
    setMutating(true)
    setError(null)
    try {
      await deleteCardContent({
        cardContentId: selected.cardContent.id,
        expectedUpdatedAt: selected.cardContent.updatedAt,
        expectedLifecycleUpdatedAt: selected.lifecycleUpdatedAt,
      })
      setConfirmingDelete(false)
      onQueueChanged()
      refresh()
    } catch (cause) {
      setError(errorMessage(cause))
      refresh()
    } finally {
      setMutating(false)
    }
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (editing || event.nativeEvent.isComposing) {
      return
    }
    if (event.metaKey && !event.altKey && !event.ctrlKey) {
      if (event.key.toLowerCase() === 'f') {
        event.preventDefault()
        searchRef.current?.focus()
        searchRef.current?.select()
      } else if (event.key.toLowerCase() === 'j') {
        event.preventDefault()
        void toggleSuspended()
      } else if (event.key === 'Backspace' && selected) {
        event.preventDefault()
        setConfirmingDelete(true)
      }
      return
    }
    if (event.metaKey || event.altKey || event.ctrlKey) {
      return
    }
    if (
      event.target instanceof Element &&
      event.target.closest('.card-detail-actions, .delete-confirmation')
    ) {
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveSelection(1)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(-1)
    } else if (event.key === 'Enter' && selected) {
      event.preventDefault()
      setEditing(true)
    } else if (event.key === 'Escape') {
      if (confirmingDelete) {
        setConfirmingDelete(false)
      } else if (query) {
        setQuery('')
      }
    }
  }

  if (editing && selected?.cardContent.type === 'BASIC') {
    return (
      <BasicCardForm
        initialContent={selected.cardContent}
        onCancel={() => setEditing(false)}
        onSaved={(item) => {
          if (item) {
            setResults((current) =>
              current.map((candidate) =>
                candidate.cardContent.id === item.cardContent.id ? item : candidate,
              ),
            )
          }
          setEditing(false)
          onQueueChanged()
          refresh()
        }}
        variant="main"
      />
    )
  }

  return (
    <section className="card-browser" onKeyDown={handleKeyDown}>
      <aside className="card-browser-sidebar">
        <div className="card-search">
          <span aria-hidden="true">⌕</span>
          <DaraInput
            aria-label="Search cards"
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search every card"
            ref={searchRef}
            type="search"
            value={query}
          />
          <kbd>⌘F</kbd>
        </div>

        <div className="card-result-summary" aria-live="polite">
          <span>{query ? 'Matches' : 'Recently edited'}</span>
          <span>{loading ? '…' : results.length}</span>
        </div>

        <div className="card-result-list" role="listbox" aria-label="Cards">
          {results.map((item) => {
            const active = item.cardContent.id === selectedId
            const suspended = item.reviewStatus === 'SUSPENDED'
            return (
              <button
                aria-selected={active}
                className={`card-result${active ? ' card-result-selected' : ''}${suspended ? ' card-result-suspended' : ''}`}
                key={item.cardContent.id}
                onClick={() => {
                  setSelectedId(item.cardContent.id)
                  setConfirmingDelete(false)
                }}
                onDoubleClick={() => setEditing(true)}
                role="option"
                type="button"
              >
                <span className="card-result-title">
                  {cardTitle(item.cardContent.frontMd)}
                </span>
                <span className="card-result-meta">
                  {suspended && <span className="suspended-badge">Paused</span>}
                  <time dateTime={new Date(item.cardContent.updatedAt).toISOString()}>
                    {formatRecency(item.cardContent.updatedAt)}
                  </time>
                </span>
              </button>
            )
          })}
          {!loading && results.length === 0 && (
            <p className="card-result-empty">No cards found.</p>
          )}
        </div>
      </aside>

      <div className="card-browser-detail">
        {selected ? (
          <>
            <header className="card-detail-toolbar">
              <div>
                <span className="card-type-label">BASIC</span>
                {selected.reviewStatus !== 'ACTIVE' && (
                  <span className="detail-status">{statusLabel(selected.reviewStatus)}</span>
                )}
              </div>
              <div className="card-detail-actions">
                <button disabled={mutating} onClick={() => void toggleSuspended()} type="button">
                  {selected.reviewStatus === 'SUSPENDED' ? 'Resume' : 'Pause'} <kbd>⌘J</kbd>
                </button>
                <button disabled={mutating} onClick={() => setEditing(true)} type="button">
                  Edit <kbd>↵</kbd>
                </button>
                <button
                  className="danger-button"
                  disabled={mutating}
                  onClick={() => setConfirmingDelete(true)}
                  type="button"
                >
                  Delete
                </button>
              </div>
            </header>

            {confirmingDelete && (
              <div className="delete-confirmation" role="alert">
                <span>Delete this card? Review history will be retained.</span>
                <div>
                  <button disabled={mutating} onClick={() => void removeSelected()} type="button">
                    {mutating ? 'Deleting…' : 'Delete card'}
                  </button>
                  <button disabled={mutating} onClick={() => setConfirmingDelete(false)} type="button">
                    Cancel
                  </button>
                </div>
              </div>
            )}

            <article className="card-detail-content">
              <section>
                <span>Front</span>
                <MarkdownRenderer source={selected.cardContent.frontMd} />
              </section>
              <section>
                <span>Back</span>
                <MarkdownRenderer source={selected.cardContent.backMd} />
              </section>
              {selected.cardContent.source && (
                <section>
                  <span>Source</span>
                  <CardSource value={selected.cardContent.source} />
                </section>
              )}
            </article>
          </>
        ) : (
          <div className="card-browser-placeholder">
            <h2>{loading ? 'Searching…' : 'No card selected'}</h2>
            <p>Start typing to search, or choose a recently edited card.</p>
          </div>
        )}
        {error && (
          <p className="card-browser-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </section>
  )
}

function cardTitle(markdown: string): string {
  const title = markdown
    .replace(/```[\s\S]*?```/g, ' code ')
    .replace(/[`*_~#[\]()>|]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
  return title || 'Untitled card'
}

function formatRecency(updatedAt: number): string {
  const elapsed = Math.max(0, Date.now() - updatedAt)
  const minutes = Math.floor(elapsed / 60_000)
  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(
    new Date(updatedAt),
  )
}

function statusLabel(status: CardContentListItem['reviewStatus']): string {
  if (status === 'SUSPENDED') return 'Paused'
  if (status === 'MIXED') return 'Partially paused'
  return 'Active'
}
