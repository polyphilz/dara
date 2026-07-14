import { listen } from '@tauri-apps/api/event'
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import { native } from '../../lib/native.ts'
import {
  ReviewController,
  tauriReviewGateway,
  type ReviewControllerState,
} from '../../review/index.ts'
import type { ReviewCardCache, ReviewGrade } from '../../scheduling/index.ts'
import { errorMessage } from '../../review/errors.ts'

const grades = [
  { grade: 1, label: 'Again' },
  { grade: 2, label: 'Hard' },
  { grade: 3, label: 'Good' },
  { grade: 4, label: 'Easy' },
] as const

export function MainWindow() {
  const controller = useMemo(
    () => new ReviewController(tauriReviewGateway),
    [],
  )
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot)
  const spaceCanSubmit = useRef(true)
  const [windowError, setWindowError] = useState<string | null>(null)

  useEffect(() => {
    void controller.start()
  }, [controller])

  useEffect(() => {
    let disposed = false
    let stopListening: (() => void) | undefined

    void listen('card-created', () => controller.notifyCardCreated()).then(
      (unlisten) => {
        if (disposed) {
          unlisten()
        } else {
          stopListening = unlisten
        }
      },
    )

    return () => {
      disposed = true
      stopListening?.()
    }
  }, [controller])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.metaKey &&
        !event.altKey &&
        !event.ctrlKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === 'z'
      ) {
        if (canUndo(state)) {
          event.preventDefault()
          void controller.undo()
        }
        return
      }

      if (event.metaKey || event.altKey || event.ctrlKey) {
        return
      }

      if (state.phase === 'QUESTION' && event.key === ' ' && !event.repeat) {
        event.preventDefault()
        spaceCanSubmit.current = false
        controller.reveal()
        return
      }

      if (state.phase !== 'REVEALED') {
        return
      }

      if (event.key === 'Tab') {
        event.preventDefault()
        controller.moveGradeFocus(event.shiftKey ? -1 : 1)
        return
      }

      if (event.repeat) {
        return
      }

      if (event.key === 'Enter') {
        event.preventDefault()
        void controller.submitFocusedGrade()
        return
      }

      if (event.key === ' ' && spaceCanSubmit.current) {
        event.preventDefault()
        void controller.submitFocusedGrade()
        return
      }

      const numericGrade = Number(event.key)
      if (isReviewGrade(numericGrade)) {
        event.preventDefault()
        void controller.submitGrade(numericGrade)
      }
    }

    const handleKeyUp = (event: KeyboardEvent) => {
      if (event.key === ' ') {
        spaceCanSubmit.current = true
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    window.addEventListener('keyup', handleKeyUp)
    return () => {
      window.removeEventListener('keydown', handleKeyDown)
      window.removeEventListener('keyup', handleKeyUp)
    }
  }, [controller, state])

  const openQuickAdd = async () => {
    setWindowError(null)
    try {
      await native.showQuickAdd()
    } catch (cause) {
      setWindowError(errorMessage(cause))
    }
  }

  return (
    <main className="main-window">
      <header className="main-toolbar">
        <h1>dara</h1>
        <div className="toolbar-actions">
          {canUndo(state) && (
            <button type="button" onClick={() => void controller.undo()}>
              Undo
            </button>
          )}
          <button type="button" onClick={() => void openQuickAdd()}>
            Add card
          </button>
        </div>
      </header>

      {windowError && (
        <p className="error-banner" role="alert">
          {windowError}
        </p>
      )}

      <ReviewContent controller={controller} state={state} />
    </main>
  )
}

function ReviewContent({
  controller,
  state,
}: {
  controller: ReviewController
  state: ReviewControllerState
}) {
  switch (state.phase) {
    case 'IDLE':
    case 'LOADING':
      return <StatusScreen message="Loading…" />
    case 'QUESTION':
      return (
        <section className="review-stage">
          {state.notice && <p className="notice">{state.notice}</p>}
          <article className="review-card">
            <CardText value={state.card.context.cardContent.frontMd} />
          </article>
          <button
            className="primary-action reveal-action"
            type="button"
            onClick={() => controller.reveal()}
          >
            Reveal answer
          </button>
          <p className="key-hint">Space to reveal</p>
        </section>
      )
    case 'REVEALED':
    case 'SUBMITTING': {
      const saving = state.phase === 'SUBMITTING'
      const content = state.card.context.cardContent
      return (
        <section className="review-stage">
          {'notice' in state && state.notice && (
            <p className="notice">{state.notice}</p>
          )}
          <article className="review-card">
            <CardText value={content.frontMd} />
            <div className="answer">
              <CardText value={content.backMd} />
              {content.source && <p className="source">Source: {content.source}</p>}
            </div>
          </article>
          <div className="grade-grid" aria-label="Grade this card">
            {grades.map(({ grade, label }) => (
              <button
                className={
                  grade === state.focusedGrade
                    ? 'grade-button grade-focused'
                    : 'grade-button'
                }
                disabled={saving}
                key={grade}
                type="button"
                onClick={() => void controller.submitGrade(grade)}
              >
                <span>{label}</span>
                <small>
                  {formatInterval(state.previews[grade].cache, state.previewedAt)}
                </small>
                <kbd>{grade}</kbd>
              </button>
            ))}
          </div>
          <p className="key-hint">
            {saving ? 'Saving…' : '1–4 to grade · Tab to choose · Enter to submit'}
          </p>
        </section>
      )
    }
    case 'UNDOING':
      return <StatusScreen message="Undoing…" />
    case 'CAUGHT_UP':
      return (
        <StatusScreen
          message="Caught up for now"
          detail={formatNextDue(state.nextDueAt)}
          notice={state.notice}
        >
          <button type="button" onClick={() => void controller.refresh()}>
            Refresh
          </button>
        </StatusScreen>
      )
    case 'ERROR':
      return (
        <StatusScreen message="Something went wrong" detail={state.message} error>
          {state.canRetry && (
            <button type="button" onClick={() => void controller.retry()}>
              Retry
            </button>
          )}
        </StatusScreen>
      )
  }
}

function CardText({ value }: { value: string }) {
  return <div className="card-text">{value}</div>
}

function StatusScreen({
  children,
  detail,
  error = false,
  message,
  notice,
}: {
  children?: ReactNode
  detail?: string | null
  error?: boolean
  message: string
  notice?: string | null
}) {
  return (
    <section className="status-screen" aria-live="polite">
      {notice && <p className="notice">{notice}</p>}
      <h2>{message}</h2>
      {detail && <p className={error ? 'status-error' : undefined}>{detail}</p>}
      {children}
    </section>
  )
}

function canUndo(state: ReviewControllerState): boolean {
  return (
    state.canUndo &&
    ['QUESTION', 'REVEALED', 'CAUGHT_UP'].includes(state.phase)
  )
}

function isReviewGrade(value: number): value is ReviewGrade {
  return Number.isInteger(value) && value >= 1 && value <= 4
}

function formatInterval(cache: ReviewCardCache, previewedAt: number): string {
  if (cache.dueAt === null) {
    const scheduledDays = cache.schedulerState?.scheduledDays
    return scheduledDays === undefined ? '—' : `${scheduledDays}d`
  }
  const minutes = Math.max(
    1,
    Math.round((cache.dueAt - previewedAt) / 60_000),
  )
  if (minutes < 60) {
    return `${minutes}m`
  }
  const hours = Math.round(minutes / 60)
  if (hours < 24) {
    return `${hours}h`
  }
  return `${Math.round(hours / 24)}d`
}

function formatNextDue(nextDueAt: number | null): string | null {
  if (nextDueAt === null) {
    return 'Nothing else is due right now.'
  }
  return `Next card due ${new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(nextDueAt))}.`
}
