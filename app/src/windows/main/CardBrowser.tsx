import { listen } from '@tauri-apps/api/event'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react'
import { DaraInput } from '../../components/DaraInput.tsx'
import { ClozeMarkdownRenderer } from '../../cloze/ClozeMarkdownRenderer.tsx'
import {
  ClozeProjection,
  clozeAnswerMarkdown,
  clozeIndexFromVariantKey,
} from '../../cloze/cloze.ts'
import { CardSource } from '../../markdown/CardSource.tsx'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer.tsx'
import { OcclusionReview } from '../../occlusion/OcclusionReview.tsx'
import { occlusionLayerId } from '../../occlusion/occlusion.ts'
import {
  CardContentReviewStatus,
  CardContentType,
  OcclusionMode,
  ReviewCardStatus,
  SearchExecutionMode,
  SemanticSearchPhase,
  deleteCardContent,
  searchCardContent,
  searchStatus,
  setCardContentSuspended,
  type CardContentListItem,
  type ReviewCardListItem,
  type SemanticSearchStatus,
} from '../../review/index.ts'
import { errorMessage } from '../../review/errors.ts'
import { ReviewCardState } from '../../scheduling/index.ts'
import { captureStudyMoment } from '../../scheduling/study-clock.ts'
import { studyDayToIsoDate } from './home-activity.ts'
import { CardForm } from '../shared/CardForm.tsx'
import { CardFormVariant } from '../shared/card-form.ts'

const SEARCH_PAGE_SIZE = 50
const SEARCH_FETCH_LIMIT = SEARCH_PAGE_SIZE + 1
const BROWSE_COMMAND_EVENT = 'browse-command'

const BrowseCommand = {
  FocusSearch: 'FOCUS_SEARCH',
  ToggleSelectedSuspension: 'TOGGLE_SELECTED_SUSPENSION',
} as const

type BrowseCommand = (typeof BrowseCommand)[keyof typeof BrowseCommand]

const browseCommands = new Set<BrowseCommand>(Object.values(BrowseCommand))

interface CardBrowserProps {
  onCardContentChanged?: () => void
  onQueueChanged: () => void
  navigationToken?: number
  refreshToken?: number
}

export function CardBrowser({
  onQueueChanged,
  onCardContentChanged = onQueueChanged,
  navigationToken = 0,
  refreshToken = 0,
}: CardBrowserProps) {
  const searchRef = useRef<HTMLInputElement>(null)
  const resultRefs = useRef(new Map<string, HTMLButtonElement>())
  const requestId = useRef(0)
  const [query, setQuery] = useState('')
  const [submittedQuery, setSubmittedQuery] = useState('')
  const [searchPending, setSearchPending] = useState(false)
  const [revision, setRevision] = useState(0)
  const [results, setResults] = useState<CardContentListItem[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [selectedReviewCardId, setSelectedReviewCardId] = useState<string | null>(
    null,
  )
  const [editing, setEditing] = useState(false)
  const [confirmingDelete, setConfirmingDelete] = useState(false)
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [hasMore, setHasMore] = useState(false)
  const [mutating, setMutating] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [searchMode, setSearchMode] = useState<SearchExecutionMode>(
    SearchExecutionMode.Browse,
  )
  const [semanticStatus, setSemanticStatus] =
    useState<SemanticSearchStatus | null>(null)

  const selected = useMemo(
    () => results.find((item) => item.cardContent.id === selectedId) ?? null,
    [results, selectedId],
  )
  const reviewCards = useMemo(
    () => (selected ? orderedReviewCards(selected) : []),
    [selected],
  )
  const selectedReviewCard =
    reviewCards.find((card) => card.id === selectedReviewCardId) ??
    reviewCards[0] ??
    null
  const selectedOcclusionLayerId =
    selected?.cardContent.type === CardContentType.Occlusion && selectedReviewCard
      ? occlusionLayerId(selectedReviewCard.variantKey)
      : null

  useEffect(() => {
    searchRef.current?.focus()
  }, [])

  useEffect(() => {
    setEditing(false)
    setConfirmingDelete(false)
  }, [navigationToken])

  useEffect(() => {
    setSelectedReviewCardId((current) =>
      current && reviewCards.some((card) => card.id === current)
        ? current
        : (reviewCards[0]?.id ?? null),
    )
  }, [reviewCards])

  useEffect(() => {
    if (searchPending) {
      return
    }
    const currentRequest = ++requestId.current
    setLoading(true)
    setLoadingMore(false)
    setHasMore(false)
    setError(null)
    void searchCardContent({
      query: submittedQuery,
      limit: SEARCH_FETCH_LIMIT,
      offset: 0,
    })
      .then((result) => {
        if (requestId.current !== currentRequest) {
          return
        }
        const nextResults = result.items
        const page = nextResults.slice(0, SEARCH_PAGE_SIZE)
        setResults(page)
        setSearchMode(result.mode)
        setSemanticStatus(result.semanticStatus)
        setHasMore(nextResults.length > SEARCH_PAGE_SIZE)
        setSelectedId((current) =>
          current && page.some((item) => item.cardContent.id === current)
            ? current
            : (page[0]?.cardContent.id ?? null),
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
  }, [submittedQuery, refreshToken, revision, searchPending])

  useEffect(() => {
    if (
      semanticStatus === null ||
      semanticStatus.phase === SemanticSearchPhase.Ready ||
      semanticStatus.phase === SemanticSearchPhase.Unavailable ||
      semanticStatus.phase === SemanticSearchPhase.Failed
    ) {
      return
    }
    const timer = window.setInterval(() => {
      void searchStatus()
        .then(setSemanticStatus)
        .catch(() => undefined)
    }, 1_000)
    return () => window.clearInterval(timer)
  }, [semanticStatus])

  const refresh = useCallback(() => {
    setRevision((value) => value + 1)
  }, [])

  const loadMore = async () => {
    if (loading || loadingMore || !hasMore) {
      return
    }
    const currentRequest = ++requestId.current
    setLoadingMore(true)
    setError(null)
    try {
      const nextResults = await searchCardContent({
        query: submittedQuery,
        limit: SEARCH_FETCH_LIMIT,
        offset: results.length,
      })
      if (requestId.current !== currentRequest) {
        return
      }
      const page = nextResults.items.slice(0, SEARCH_PAGE_SIZE)
      setSearchMode(nextResults.mode)
      setSemanticStatus(nextResults.semanticStatus)
      setResults((current) => {
        const existingIds = new Set(
          current.map((item) => item.cardContent.id),
        )
        return [
          ...current,
          ...page.filter((item) => !existingIds.has(item.cardContent.id)),
        ]
      })
      setHasMore(nextResults.items.length > SEARCH_PAGE_SIZE)
    } catch (cause) {
      if (requestId.current === currentRequest) {
        setError(errorMessage(cause))
      }
    } finally {
      if (requestId.current === currentRequest) {
        setLoadingMore(false)
      }
    }
  }

  const moveSelection = (delta: -1 | 1, moveFocus = false) => {
    if (results.length === 0) {
      return
    }
    const currentIndex = Math.max(
      0,
      results.findIndex((item) => item.cardContent.id === selectedId),
    )
    const nextIndex = Math.min(results.length - 1, Math.max(0, currentIndex + delta))
    const nextId = results[nextIndex]!.cardContent.id
    setSelectedId(nextId)
    setConfirmingDelete(false)
    if (moveFocus) {
      resultRefs.current.get(nextId)?.focus()
    }
  }

  const clearSearch = () => {
    requestId.current += 1
    setQuery('')
    setSubmittedQuery('')
    setSearchPending(false)
    setResults([])
    setSelectedId(null)
    setHasMore(false)
    setRevision((value) => value + 1)
  }

  const updateQuery = (value: string) => {
    setQuery(value)
    if (value.trim()) {
      setSearchPending(true)
      requestId.current += 1
      setResults([])
      setSelectedId(null)
      setHasMore(false)
      setLoading(false)
      setError(null)
      return
    }
    clearSearch()
  }

  const submitSearch = () => {
    if (!query.trim()) {
      clearSearch()
      return
    }
    setSubmittedQuery(query)
    setSearchPending(false)
    setRevision((value) => value + 1)
  }

  const focusSearch = useCallback(() => {
    searchRef.current?.focus()
    searchRef.current?.select()
  }, [])

  const toggleSuspended = useCallback(async () => {
    if (!selected || mutating) {
      return
    }
    setMutating(true)
    setError(null)
    try {
      const item = await setCardContentSuspended({
        cardContentId: selected.cardContent.id,
        expectedLifecycleUpdatedAt: selected.lifecycleUpdatedAt,
        suspended:
          selected.reviewStatus !== CardContentReviewStatus.Suspended,
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
  }, [mutating, onQueueChanged, refresh, selected])

  const runBrowseCommand = useCallback(
    (command: BrowseCommand) => {
      if (editing) {
        return
      }
      switch (command) {
        case BrowseCommand.FocusSearch:
          focusSearch()
          return
        case BrowseCommand.ToggleSelectedSuspension:
          void toggleSuspended()
          return
        default:
          return command satisfies never
      }
    },
    [editing, focusSearch, toggleSuspended],
  )
  const runBrowseCommandRef = useRef(runBrowseCommand)
  runBrowseCommandRef.current = runBrowseCommand

  useEffect(() => {
    const handleShortcut = (event: globalThis.KeyboardEvent) => {
      if (
        event.isComposing ||
        event.repeat ||
        !event.metaKey ||
        event.altKey ||
        event.ctrlKey ||
        event.shiftKey
      ) {
        return
      }
      const command =
        event.code === 'KeyF'
          ? BrowseCommand.FocusSearch
          : event.code === 'KeyJ'
            ? BrowseCommand.ToggleSelectedSuspension
            : null
      if (command === null) {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      runBrowseCommandRef.current(command)
    }
    window.addEventListener('keydown', handleShortcut, { capture: true })
    return () =>
      window.removeEventListener('keydown', handleShortcut, { capture: true })
  }, [])

  useEffect(() => {
    let disposed = false
    let stopListening: (() => void) | undefined
    void listen<unknown>(BROWSE_COMMAND_EVENT, (event) => {
      if (isBrowseCommand(event.payload)) {
        runBrowseCommandRef.current(event.payload)
      }
    }).then((unlisten) => {
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
      if (event.key === 'Backspace' && selected) {
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
      moveSelection(1, event.target instanceof Element && !!event.target.closest('.card-result'))
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(-1, event.target instanceof Element && !!event.target.closest('.card-result'))
    } else if (event.key === 'Enter' && selected) {
      event.preventDefault()
      setEditing(true)
    } else if (event.key === 'Escape') {
      if (confirmingDelete) {
        setConfirmingDelete(false)
      } else if (query) {
        clearSearch()
        searchRef.current?.focus()
      }
    }
  }

  const handleSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) {
      return
    }
    if (event.key === 'Enter' && !event.metaKey && !event.altKey && !event.ctrlKey) {
      event.preventDefault()
      event.stopPropagation()
      submitSearch()
    } else if (event.key === 'ArrowDown' && results.length > 0) {
      event.preventDefault()
      event.stopPropagation()
      const resultId = selectedId ?? results[0]!.cardContent.id
      setSelectedId(resultId)
      resultRefs.current.get(resultId)?.focus()
    }
  }

  if (editing && selected) {
    return (
      <CardForm
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
          onCardContentChanged()
          refresh()
        }}
        variant={CardFormVariant.Main}
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
            onChange={(event) => updateQuery(event.target.value)}
            onKeyDown={handleSearchKeyDown}
            placeholder="Search every card · Enter"
            ref={searchRef}
            type="search"
            value={query}
          />
          <kbd>⌘F</kbd>
        </div>

        <div className="card-result-summary" aria-live="polite">
          <span>{searchSummaryLabel(searchPending, searchMode)}</span>
          <span>
            {searchPending ? '↵' : loading ? '…' : `${results.length}${hasMore ? '+' : ''}`}
          </span>
        </div>

        {semanticStatus && semanticStatus.phase !== SemanticSearchPhase.Ready && (
          <p
            className={`semantic-search-status${semanticStatusShimmers(semanticStatus) ? ' semantic-search-status-shimmering' : ''}`}
            data-shimmer-text={
              semanticStatusShimmers(semanticStatus)
                ? semanticStatusLabel(semanticStatus)
                : undefined
            }
            role="status"
          >
            {semanticStatusLabel(semanticStatus)}
          </p>
        )}

        <div className="card-result-list" role="listbox" aria-label="Cards">
          {results.map((item) => {
            const active = item.cardContent.id === selectedId
            const suspended =
              item.reviewStatus === CardContentReviewStatus.Suspended
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
                onFocus={() => setSelectedId(item.cardContent.id)}
                ref={(element) => {
                  if (element) {
                    resultRefs.current.set(item.cardContent.id, element)
                  } else {
                    resultRefs.current.delete(item.cardContent.id)
                  }
                }}
                role="option"
                type="button"
              >
                <span className="card-result-title">
                  {cardTitle(
                    item.cardContent.type === CardContentType.Cloze
                      ? clozeAnswerMarkdown(item.cardContent.frontMd)
                      : item.cardContent.type === CardContentType.Occlusion
                        ? item.cardContent.frontMd ||
                          `Image occlusion · ${item.cardContent.occlusion.layers.length} layers`
                        : item.cardContent.frontMd,
                  )}
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
            <p className="card-result-empty">
              {searchPending ? 'Press Enter to search.' : 'No cards found.'}
            </p>
          )}
        </div>
        {hasMore && (
          <button
            className="card-browser-load-more"
            disabled={loadingMore}
            onClick={() => void loadMore()}
            type="button"
          >
            {loadingMore ? 'Loading…' : 'Load more'}
          </button>
        )}
      </aside>

      <div className="card-browser-detail">
        {selected ? (
          <>
            <header className="card-detail-toolbar">
              <div>
                <span className="card-type-label">
                  {selected.cardContent.type}
                </span>
                {selected.cardContent.type === CardContentType.Occlusion && (
                  <span className="occlusion-mode-badge">
                    {occlusionModeLabel(selected.cardContent.occlusion.mode)}
                  </span>
                )}
                {selected.reviewStatus !== CardContentReviewStatus.Active && (
                  <span className="detail-status">{statusLabel(selected.reviewStatus)}</span>
                )}
              </div>
              <div className="card-detail-actions">
                <button disabled={mutating} onClick={() => void toggleSuspended()} type="button">
                  {selected.reviewStatus === CardContentReviewStatus.Suspended
                    ? 'Resume'
                    : 'Pause'}{' '}
                  <kbd>⌘J</kbd>
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
              {selected.cardContent.type === CardContentType.Occlusion ? (
                <>
                  <section>
                    <span>
                      Image · {selected.cardContent.occlusion.layers.length} layers
                    </span>
                    {selectedOcclusionLayerId && (
                      <div className="occlusion-browser-preview">
                        <OcclusionReview
                          definition={selected.cardContent.occlusion}
                          revealed={false}
                          targetLayerId={selectedOcclusionLayerId}
                        />
                      </div>
                    )}
                  </section>
                  {selected.cardContent.frontMd.trim() && (
                    <section>
                      <span>Prompt</span>
                      <MarkdownRenderer source={selected.cardContent.frontMd} />
                    </section>
                  )}
                </>
              ) : (
                <section>
                  <span>
                    {selected.cardContent.type === CardContentType.Basic
                      ? 'Front'
                      : 'Text'}
                  </span>
                  {selected.cardContent.type === CardContentType.Basic ? (
                    <MarkdownRenderer source={selected.cardContent.frontMd} />
                  ) : (
                    <ClozeMarkdownRenderer
                      projection={ClozeProjection.Question}
                      source={selected.cardContent.frontMd}
                      variantKey={selectedReviewCard?.variantKey}
                    />
                  )}
                </section>
              )}
              {(selected.cardContent.type === CardContentType.Basic ||
                selected.cardContent.backMd.trim()) && (
                  <section>
                    <span>
                      {selected.cardContent.type === CardContentType.Basic
                        ? 'Back'
                        : 'Extra'}
                    </span>
                    <MarkdownRenderer source={selected.cardContent.backMd} />
                  </section>
                )}
              {reviewCards.length > 0 && (
                <section>
                  <span>Review cards · {reviewCards.length}</span>
                  <div
                    aria-label="Review cards"
                    className="review-card-siblings"
                    role="group"
                  >
                    {reviewCards.map((reviewCard, index) => {
                      const active = reviewCard.id === selectedReviewCard?.id
                      return (
                        <button
                          aria-pressed={active}
                          className={`review-card-sibling${active ? ' review-card-sibling-selected' : ''}`}
                          key={reviewCard.id}
                          onClick={() =>
                            setSelectedReviewCardId(reviewCard.id)
                          }
                          type="button"
                        >
                          <span className="review-card-sibling-number">
                            {index + 1}
                          </span>
                          <span className="review-card-sibling-name">
                            {reviewCardLabel(
                              selected.cardContent,
                              reviewCard.variantKey,
                            )}
                            {reviewCard.status === ReviewCardStatus.Suspended && (
                              <small>Paused</small>
                            )}
                          </span>
                          <span className="review-card-sibling-date">
                            <small>Due</small>
                            {formatReviewCardDue(reviewCard)}
                          </span>
                          <span className="review-card-sibling-date">
                            <small>Last reviewed</small>
                            {reviewCard.lastReviewAt === null
                              ? 'Never'
                              : formatRecency(reviewCard.lastReviewAt)}
                          </span>
                        </button>
                      )
                    })}
                  </div>
                </section>
              )}
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
            <p>Start typing to search, or choose a card.</p>
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

function isBrowseCommand(value: unknown): value is BrowseCommand {
  return typeof value === 'string' && browseCommands.has(value as BrowseCommand)
}

function searchSummaryLabel(searchPending: boolean, mode: SearchExecutionMode): string {
  if (searchPending) {
    return 'Ready to search'
  }
  switch (mode) {
    case SearchExecutionMode.Browse:
      return 'All cards'
    case SearchExecutionMode.Lexical:
      return 'Lexical matches'
    case SearchExecutionMode.Hybrid:
      return 'Hybrid matches'
    default:
      return mode satisfies never
  }
}

function semanticStatusLabel(status: SemanticSearchStatus): string {
  switch (status.phase) {
    case SemanticSearchPhase.Downloading: {
      const percent =
        status.modelBytes > 0
          ? Math.min(100, Math.floor((status.downloadedBytes / status.modelBytes) * 100))
          : 0
      return `Preparing semantic search · ${percent}%`
    }
    case SemanticSearchPhase.Verifying:
      return 'Verifying semantic search…'
    case SemanticSearchPhase.Starting:
      return 'Starting semantic search…'
    case SemanticSearchPhase.Indexing:
      return status.totalDocuments > 0
        ? `Indexing cards · ${status.indexedDocuments}/${status.totalDocuments}`
        : 'Preparing the semantic index…'
    case SemanticSearchPhase.Unavailable:
      return 'Semantic search unavailable · lexical search still works'
    case SemanticSearchPhase.Failed:
      return 'Semantic search needs attention · lexical search still works'
    case SemanticSearchPhase.Ready:
      return 'Semantic search ready'
    default:
      return status.phase satisfies never
  }
}

function semanticStatusShimmers(status: SemanticSearchStatus): boolean {
  return (
    status.phase === SemanticSearchPhase.Verifying ||
    status.phase === SemanticSearchPhase.Starting
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

function occlusionModeLabel(mode: OcclusionMode): string {
  switch (mode) {
    case OcclusionMode.HideOneGuessOne:
      return 'Hide one, guess one'
    case OcclusionMode.HideAllGuessOne:
      return 'Hide all, guess one'
  }
}

function orderedReviewCards(item: CardContentListItem): ReviewCardListItem[] {
  return [...item.reviewCards].sort((left, right) => {
    const leftIndex = reviewCardIndex(item, left.variantKey)
    const rightIndex = reviewCardIndex(item, right.variantKey)
    return leftIndex - rightIndex || left.variantKey.localeCompare(right.variantKey)
  })
}

function reviewCardIndex(
  item: CardContentListItem,
  variantKey: string,
): number {
  if (item.cardContent.type === CardContentType.Basic) {
    return 0
  }
  if (item.cardContent.type === CardContentType.Cloze) {
    const index = Number(clozeIndexFromVariantKey(variantKey))
    return Number.isSafeInteger(index) ? index : Number.MAX_SAFE_INTEGER
  }
  const layerId = occlusionLayerId(variantKey)
  const index = item.cardContent.occlusion.layers.findIndex(
    (layer) => layer.id === layerId,
  )
  return index === -1 ? Number.MAX_SAFE_INTEGER : index
}

function reviewCardLabel(
  content: CardContentListItem['cardContent'],
  variantKey: string,
): string {
  if (content.type === CardContentType.Basic) {
    return 'Basic card'
  }
  if (content.type === CardContentType.Cloze) {
    const clozeIndex = clozeIndexFromVariantKey(variantKey)
    return `Cloze ${clozeIndex}`
  }
  const layerId = occlusionLayerId(variantKey)
  const index = content.occlusion.layers.findIndex(
    (layer) => layer.id === layerId,
  )
  const layer = content.occlusion.layers[index]
  return layer?.label?.trim() || `Layer ${index + 1}`
}

function formatReviewCardDue(card: ReviewCardListItem): string {
  if (card.status === ReviewCardStatus.Suspended) {
    return 'Paused'
  }
  if (card.state === ReviewCardState.New) {
    return 'New'
  }
  if (card.dueAt !== null) {
    const remaining = card.dueAt - Date.now()
    if (remaining <= 0) return 'Now'
    const minutes = Math.ceil(remaining / 60_000)
    if (minutes < 60) return `in ${minutes}m`
    const hours = Math.ceil(remaining / 3_600_000)
    if (hours < 24) return `in ${hours}h`
    return formatDate(card.dueAt)
  }
  if (card.dueStudyDay !== null) {
    const currentStudyDay = captureStudyMoment().studyDay
    if (card.dueStudyDay <= currentStudyDay) return 'Today'
    if (card.dueStudyDay === currentStudyDay + 1) return 'Tomorrow'
    return formatStudyDay(card.dueStudyDay)
  }
  return 'Unscheduled'
}

function formatStudyDay(studyDay: number): string {
  const [year, month, day] = studyDayToIsoDate(studyDay)
    .split('-')
    .map(Number)
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
  }).format(new Date(year!, month! - 1, day!))
}

function formatDate(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
  }).format(new Date(timestamp))
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
  return formatDate(updatedAt)
}

function statusLabel(status: CardContentListItem['reviewStatus']): string {
  if (status === CardContentReviewStatus.Suspended) return 'Paused'
  if (status === CardContentReviewStatus.Mixed) return 'Partially paused'
  return 'Active'
}
