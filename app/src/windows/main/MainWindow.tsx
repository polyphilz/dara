import { listen } from '@tauri-apps/api/event'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import {
  CardContentType,
  ReviewController,
  ReviewControllerPhase,
  tauriReviewGateway,
  type CardContent,
  type ReviewControllerState,
} from '../../review/index.ts'
import type { ReviewCardCache, ReviewGrade } from '../../scheduling/index.ts'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer.tsx'
import { CardSource } from '../../markdown/CardSource.tsx'
import { ClozeMarkdownRenderer } from '../../cloze/ClozeMarkdownRenderer.tsx'
import { ClozeProjection } from '../../cloze/cloze.ts'
import { OcclusionReview } from '../../occlusion/OcclusionReview.tsx'
import { occlusionLayerId } from '../../occlusion/occlusion.ts'
import { CardForm } from '../shared/CardForm.tsx'
import { CardFormVariant } from '../shared/card-form.ts'
import { CardBrowser } from './CardBrowser.tsx'
import { Home } from './Home.tsx'
import { invalidateHomeStats } from './home-stats-cache.ts'
import { MainNavigation } from './MainNavigation.tsx'
import { Settings } from './Settings.tsx'

const grades = [
  { grade: 1, label: 'Again' },
  { grade: 2, label: 'Hard' },
  { grade: 3, label: 'Good' },
  { grade: 4, label: 'Easy' },
] as const

const MAX_TIMER_DELAY = 2_147_000_000

const MainWindowMode = {
  Home: 'HOME',
  Review: 'REVIEW',
  Browse: 'BROWSE',
  Create: 'CREATE',
  Settings: 'SETTINGS',
} as const

type MainWindowMode =
  (typeof MainWindowMode)[keyof typeof MainWindowMode]

export function MainWindow() {
  const [mode, setMode] = useState<MainWindowMode>(MainWindowMode.Home)
  const [browseNavigationToken, setBrowseNavigationToken] = useState(0)
  const [browseRefreshToken, setBrowseRefreshToken] = useState(0)
  const [homeRefreshToken, setHomeRefreshToken] = useState(0)
  const [settingsNavigationToken, setSettingsNavigationToken] = useState(0)
  const [settingsBusy, setSettingsBusy] = useState(false)
  const refreshHomeStats = useCallback(() => {
    invalidateHomeStats()
    setHomeRefreshToken((value) => value + 1)
  }, [])
  const controller = useMemo(
    () =>
      new ReviewController(tauriReviewGateway, {
        onReviewDataChanged: refreshHomeStats,
      }),
    [refreshHomeStats],
  )
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot)
  const spaceCanSubmit = useRef(true)

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
    const timer = window.setTimeout(() => {
      void controller.notifyClockChanged()
    }, delay)
    return () => window.clearTimeout(timer)
  }, [controller, state])

  useEffect(() => {
    const refreshClock = () => {
      if (document.visibilityState !== 'visible') {
        return
      }
      void controller.notifyClockChanged()
      setHomeRefreshToken((value) => value + 1)
    }
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
      refreshHomeStats()
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
  }, [controller, refreshHomeStats])

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

  const queueChanged = useCallback(() => {
    void controller.refresh()
    refreshHomeStats()
  }, [controller, refreshHomeStats])

  const cardContentChanged = useCallback(() => {
    void controller.refresh()
  }, [controller])

  const showHome = useCallback(() => setMode(MainWindowMode.Home), [])
  const showCreate = useCallback(() => setMode(MainWindowMode.Create), [])
  const showBrowse = useCallback(() => {
    setMode(MainWindowMode.Browse)
    setBrowseNavigationToken((value) => value + 1)
  }, [])
  const showReview = useCallback(() => {
    if (settingsBusy) {
      return
    }
    setMode(MainWindowMode.Review)
    void controller.refresh()
  }, [controller, settingsBusy])
  const showSettings = useCallback(() => {
    if (settingsBusy) {
      return
    }
    setMode(MainWindowMode.Settings)
    setSettingsNavigationToken((value) => value + 1)
  }, [settingsBusy])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.metaKey &&
        !event.altKey &&
        !event.ctrlKey &&
        !event.shiftKey &&
        event.code === 'Comma'
      ) {
        event.preventDefault()
        showSettings()
      }
    }
    window.addEventListener('keydown', handleKeyDown)

    let disposed = false
    const listeners = Promise.all([
      listen('open-settings', showSettings),
      listen('open-home', showHome),
    ]).then((unlisteners) => {
      if (disposed) {
        unlisteners.forEach((unlisten) => unlisten())
        return []
      }
      return unlisteners
    })
    return () => {
      disposed = true
      window.removeEventListener('keydown', handleKeyDown)
      void listeners.then((unlisteners) => {
        unlisteners.forEach((unlisten) => unlisten())
      })
    }
  }, [showHome, showSettings])

  const schedulingChanged = useCallback(() => {
    void controller.refresh()
    refreshHomeStats()
    setBrowseRefreshToken((value) => value + 1)
  }, [controller, refreshHomeStats])

  return (
    <main
      className={`main-window${mode === MainWindowMode.Home ? ' main-window-home' : ''}${mode === MainWindowMode.Create ? ' main-window-creating' : ''}${mode === MainWindowMode.Browse ? ' main-window-browsing' : ''}${mode === MainWindowMode.Settings ? ' main-window-settings' : ''}`}
    >
      <header className="main-header">
        <MainNavigation
          disabled={settingsBusy}
          onAdd={showCreate}
          onBrowse={showBrowse}
          onHome={showHome}
          onSettings={showSettings}
        />
      </header>

      <div hidden={mode !== MainWindowMode.Home}>
        <Home
          onReview={showReview}
          refreshToken={homeRefreshToken}
        />
      </div>

      {mode === MainWindowMode.Create ? (
        <CardForm
          onCancel={showHome}
          onSaved={() => {
            controller.notifyCardCreated()
            refreshHomeStats()
            showHome()
          }}
          variant={CardFormVariant.Main}
        />
      ) : mode === MainWindowMode.Browse ? (
        <CardBrowser
          navigationToken={browseNavigationToken}
          onCardContentChanged={cardContentChanged}
          onQueueChanged={queueChanged}
          refreshToken={browseRefreshToken}
        />
      ) : mode === MainWindowMode.Review ? (
        <ReviewContent controller={controller} state={state} />
      ) : mode === MainWindowMode.Settings ? (
        <Settings
          navigationToken={settingsNavigationToken}
          onBusyChange={setSettingsBusy}
          onSchedulingChanged={schedulingChanged}
          reviewSaveInFlight={state.phase === ReviewControllerPhase.Submitting}
        />
      ) : null}
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
            <CardQuestion
              content={state.card.context.cardContent}
              variantKey={state.card.context.reviewCard.variantKey}
            />
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
            <CardAnswer
              content={content}
              variantKey={state.card.context.reviewCard.variantKey}
            />
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

function CardQuestion({
  content,
  variantKey,
}: {
  content: CardContent
  variantKey: string
}) {
  switch (content.type) {
    case CardContentType.Basic:
      return <MarkdownRenderer source={content.frontMd} />
    case CardContentType.Cloze:
      return (
        <ClozeMarkdownRenderer
          projection={ClozeProjection.Question}
          source={content.frontMd}
          variantKey={variantKey}
        />
      )
    case CardContentType.Occlusion: {
      const targetLayerId = occlusionLayerId(variantKey)
      return (
        <>
          {content.frontMd.trim() && <MarkdownRenderer source={content.frontMd} />}
          {targetLayerId && (
            <OcclusionReview
              definition={content.occlusion}
              revealed={false}
              targetLayerId={targetLayerId}
            />
          )}
        </>
      )
    }
  }
}

function CardAnswer({
  content,
  variantKey,
}: {
  content: CardContent
  variantKey: string
}) {
  switch (content.type) {
    case CardContentType.Basic:
      return (
        <>
          <MarkdownRenderer source={content.frontMd} />
          <div className="answer">
            <MarkdownRenderer source={content.backMd} />
            {content.source && <CardSource value={content.source} />}
          </div>
        </>
      )
    case CardContentType.Cloze:
      return (
        <>
          <ClozeMarkdownRenderer
            projection={ClozeProjection.Answer}
            source={content.frontMd}
          />
          {(content.backMd.trim() || content.source) && (
            <div className="answer">
              {content.backMd.trim() && (
                <MarkdownRenderer source={content.backMd} />
              )}
              {content.source && <CardSource value={content.source} />}
            </div>
          )}
        </>
      )
    case CardContentType.Occlusion: {
      const targetLayerId = occlusionLayerId(variantKey)
      return (
        <>
          {content.frontMd.trim() && <MarkdownRenderer source={content.frontMd} />}
          {targetLayerId && (
            <OcclusionReview
              definition={content.occlusion}
              revealed
              targetLayerId={targetLayerId}
            />
          )}
          {(content.backMd.trim() || content.source) && (
            <div className="answer">
              {content.backMd.trim() && <MarkdownRenderer source={content.backMd} />}
              {content.source && <CardSource value={content.source} />}
            </div>
          )}
        </>
      )
    }
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
