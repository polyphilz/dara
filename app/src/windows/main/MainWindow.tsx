import { listen } from '@tauri-apps/api/event'
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import {
  ReviewController,
  ReviewControllerPhase,
  tauriReviewGateway,
  type ReviewControllerState,
} from '../../review/index.ts'
import type { ReviewCardCache, ReviewGrade } from '../../scheduling/index.ts'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer.tsx'
import { CardSource } from '../../markdown/CardSource.tsx'
import { BasicCardForm } from '../shared/BasicCardForm.tsx'
import { BasicCardFormVariant } from '../shared/card-form.ts'
import { CardBrowser } from './CardBrowser.tsx'

const grades = [
  { grade: 1, label: 'Again' },
  { grade: 2, label: 'Hard' },
  { grade: 3, label: 'Good' },
  { grade: 4, label: 'Easy' },
] as const

const MAX_TIMER_DELAY = 2_147_000_000

const MainWindowMode = {
  Review: 'REVIEW',
  Browse: 'BROWSE',
  Create: 'CREATE',
} as const

type MainWindowMode =
  (typeof MainWindowMode)[keyof typeof MainWindowMode]

export function MainWindow() {
  const controller = useMemo(
    () => new ReviewController(tauriReviewGateway),
    [],
  )
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot)
  const spaceCanSubmit = useRef(true)
  const [mode, setMode] = useState<MainWindowMode>(MainWindowMode.Review)
  const [browseRefreshToken, setBrowseRefreshToken] = useState(0)

  useEffect(() => {
    void controller.start()
  }, [controller])

  useEffect(() => {
    if (
      state.phase !== ReviewControllerPhase.CaughtUp ||
      state.nextDueAt === null
    ) {
      return
    }
    const delay = Math.min(
      MAX_TIMER_DELAY,
      Math.max(0, state.nextDueAt - Date.now() + 25),
    )
    const timer = window.setTimeout(() => controller.notifyClockChanged(), delay)
    return () => window.clearTimeout(timer)
  }, [controller, state])

  useEffect(() => {
    const refreshClock = () => controller.notifyClockChanged()
    window.addEventListener('focus', refreshClock)
    document.addEventListener('visibilitychange', refreshClock)

    let disposed = false
    let stopListening: (() => void) | undefined
    void listen('review-clock-refresh', refreshClock).then((unlisten) => {
      if (disposed) {
        unlisten()
      } else {
        stopListening = unlisten
      }
    })

    return () => {
      disposed = true
      stopListening?.()
      window.removeEventListener('focus', refreshClock)
      document.removeEventListener('visibilitychange', refreshClock)
    }
  }, [controller])

  useEffect(() => {
    let disposed = false
    let stopListening: (() => void) | undefined

    void listen('card-created', () => {
      controller.notifyCardCreated()
      setBrowseRefreshToken((value) => value + 1)
    }).then(
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
      if (mode !== MainWindowMode.Review) {
        return
      }
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

      if (
        state.phase === ReviewControllerPhase.Question &&
        event.key === ' ' &&
        !event.repeat
      ) {
        event.preventDefault()
        spaceCanSubmit.current = false
        controller.reveal()
        return
      }

      if (state.phase !== ReviewControllerPhase.Revealed) {
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
  }, [controller, mode, state])

  const queueChanged = () => {
    void controller.refresh()
  }

  return (
    <main
      className={`main-window${mode === MainWindowMode.Create ? ' main-window-creating' : ''}${mode === MainWindowMode.Browse ? ' main-window-browsing' : ''}`}
    >
      <header className="main-toolbar">
        <div className="main-toolbar-leading">
          <h1>dara</h1>
          {mode !== MainWindowMode.Create && (
            <nav aria-label="Main sections" className="main-section-tabs">
              <button
                aria-current={mode === MainWindowMode.Review ? 'page' : undefined}
                onClick={() => {
                  setMode(MainWindowMode.Review)
                  void controller.refresh()
                }}
                type="button"
              >
                Review
              </button>
              <button
                aria-current={mode === MainWindowMode.Browse ? 'page' : undefined}
                onClick={() => setMode(MainWindowMode.Browse)}
                type="button"
              >
                Browse
              </button>
            </nav>
          )}
        </div>
        <div className="toolbar-actions">
          {mode === MainWindowMode.Review && canUndo(state) && (
            <button type="button" onClick={() => void controller.undo()}>
              Undo
            </button>
          )}
          {mode !== MainWindowMode.Create && (
            <button type="button" onClick={() => setMode(MainWindowMode.Create)}>
              Add card
            </button>
          )}
        </div>
      </header>

      {mode === MainWindowMode.Create ? (
        <BasicCardForm
          onCancel={() => setMode(MainWindowMode.Review)}
          onSaved={() => {
            controller.notifyCardCreated()
            setMode(MainWindowMode.Review)
          }}
          variant={BasicCardFormVariant.Main}
        />
      ) : mode === MainWindowMode.Browse ? (
        <CardBrowser
          onQueueChanged={queueChanged}
          refreshToken={browseRefreshToken}
        />
      ) : (
        <ReviewContent controller={controller} state={state} />
      )}
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
    case ReviewControllerPhase.Idle:
    case ReviewControllerPhase.Loading:
      return <StatusScreen message="Loading…" />
    case ReviewControllerPhase.Question:
      return (
        <section className="review-stage">
          {state.notice && <p className="notice">{state.notice}</p>}
          <article className="review-card">
            <MarkdownRenderer source={state.card.context.cardContent.frontMd} />
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
    case ReviewControllerPhase.Revealed:
    case ReviewControllerPhase.Submitting: {
      const saving = state.phase === ReviewControllerPhase.Submitting
      const content = state.card.context.cardContent
      return (
        <section className="review-stage">
          {'notice' in state && state.notice && (
            <p className="notice">{state.notice}</p>
          )}
          <article className="review-card">
            <MarkdownRenderer source={content.frontMd} />
            <div className="answer">
              <MarkdownRenderer source={content.backMd} />
              {content.source && <CardSource value={content.source} />}
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
    case ReviewControllerPhase.Undoing:
      return <StatusScreen message="Undoing…" />
    case ReviewControllerPhase.CaughtUp:
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
    case ReviewControllerPhase.Error:
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
    undoablePhases.has(state.phase)
  )
}

const undoablePhases = new Set<ReviewControllerState['phase']>([
  ReviewControllerPhase.Question,
  ReviewControllerPhase.Revealed,
  ReviewControllerPhase.CaughtUp,
])

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
