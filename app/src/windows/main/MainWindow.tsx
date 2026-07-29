import { listen } from '@tauri-apps/api/event'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  useBlocker,
  useMatchRoute,
  useNavigate,
  type RouterHistory,
} from '@tanstack/react-router'
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
  ReviewKeyboardActionKind,
  interpretReviewKeyDown,
  interpretReviewKeyUp,
  tauriReviewGateway,
  type CardContent,
  type ReviewControllerState,
} from '../../review/index.ts'
import {
  nextStudyDayBoundary,
  type ReviewCardCache,
} from '../../scheduling/index.ts'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer.tsx'
import { CardSource } from '../../markdown/CardSource.tsx'
import { ClozeMarkdownRenderer } from '../../cloze/ClozeMarkdownRenderer.tsx'
import { ClozeProjection } from '../../cloze/cloze.ts'
import { DaraButton } from '../../components/DaraButton.tsx'
import { DaraButtonVariant } from '../../components/dara-button-types.ts'
import { OcclusionReview } from '../../occlusion/OcclusionReview.tsx'
import { loadOffsiteBackupTakeoverRequired } from '../../backup/index.ts'
import { DaraEvent } from '../../lib/tauri-contracts.ts'
import { occlusionLayerId } from '../../occlusion/occlusion.ts'
import { CardBrowser } from './CardBrowser.tsx'
import { Home } from './Home.tsx'
import { invalidateHomeStats } from './home-stats-cache.ts'
import { MainNavigation } from './MainNavigation.tsx'
import { RoutedCardForm } from './RoutedCardForm.tsx'
import { Settings } from './Settings.tsx'
import { MainWindowRoutePath } from './main-window-routes.ts'

const grades = [
  { grade: 1, label: 'Again' },
  { grade: 2, label: 'Hard' },
  { grade: 3, label: 'Good' },
  { grade: 4, label: 'Easy' },
] as const

const MAX_TIMER_DELAY = 2_147_000_000
const CLOCK_REFRESH_GRACE = 25

const MainWindowMode = {
  Home: 'HOME',
  Review: 'REVIEW',
  Browse: 'BROWSE',
  Create: 'CREATE',
  Settings: 'SETTINGS',
} as const

type MainWindowMode =
  (typeof MainWindowMode)[keyof typeof MainWindowMode]

const rootRoute = createRootRoute({
  component: MainWindowContent,
})

const routeTree = rootRoute.addChildren([
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.Home,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.Add,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.Browse,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.BrowseCard,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.BrowseEdit,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.Review,
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: MainWindowRoutePath.Settings,
  }),
])

function createMainWindowRouter(
  history: RouterHistory = createMemoryHistory({
    initialEntries: [MainWindowRoutePath.Home],
  }),
) {
  return createRouter({ history, routeTree })
}

type MainWindowRouter = ReturnType<typeof createMainWindowRouter>

declare module '@tanstack/react-router' {
  interface Register {
    router: MainWindowRouter
  }
}

interface MainWindowProps {
  history?: RouterHistory
}

export function MainWindow({ history }: MainWindowProps = {}) {
  const [router] = useState(() => createMainWindowRouter(history))
  return <RouterProvider router={router} />
}

function MainWindowContent() {
  const matchRoute = useMatchRoute()
  const navigate = useNavigate()
  const editRouteParams = matchRoute({
    fuzzy: false,
    to: MainWindowRoutePath.BrowseEdit,
  })
  const browseCardRouteParams = matchRoute({
    fuzzy: false,
    to: MainWindowRoutePath.BrowseCard,
  })
  const editingCardContentId = editRouteParams
    ? editRouteParams.cardContentId
    : null
  const mode: MainWindowMode = editRouteParams
    ? MainWindowMode.Browse
    : browseCardRouteParams
      ? MainWindowMode.Browse
      : matchRoute({ fuzzy: false, to: MainWindowRoutePath.Add })
        ? MainWindowMode.Create
        : matchRoute({ fuzzy: false, to: MainWindowRoutePath.Browse })
          ? MainWindowMode.Browse
          : matchRoute({ fuzzy: false, to: MainWindowRoutePath.Review })
            ? MainWindowMode.Review
            : matchRoute({ fuzzy: false, to: MainWindowRoutePath.Settings })
              ? MainWindowMode.Settings
              : MainWindowMode.Home
  const [browseNavigationToken, setBrowseNavigationToken] = useState(0)
  const [browseRefreshToken, setBrowseRefreshToken] = useState(0)
  const [homeRefreshToken, setHomeRefreshToken] = useState(0)
  const [settingsNavigationToken, setSettingsNavigationToken] = useState(0)
  const [settingsBusy, setSettingsBusy] = useState(false)
  const [restoredBackupTakeoverAvailable, setRestoredBackupTakeoverAvailable] =
    useState(false)
  const [clockScheduleToken, setClockScheduleToken] = useState(0)
  const shouldBlockSettingsNavigation = useCallback(
    () => settingsBusy,
    [settingsBusy],
  )
  useBlocker({
    disabled: !settingsBusy,
    enableBeforeUnload: settingsBusy,
    shouldBlockFn: shouldBlockSettingsNavigation,
  })
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
  const previousMode = useRef(mode)
  const spaceCanSubmit = useRef(true)

  useEffect(() => {
    void controller.start()
  }, [controller])

  useEffect(() => {
    const priorMode = previousMode.current
    previousMode.current = mode
    if (
      mode === MainWindowMode.Review &&
      priorMode !== MainWindowMode.Review
    ) {
      void controller.refresh()
    }
  }, [controller, mode])

  useEffect(() => {
    const nextLearningDueAt =
      state.phase === ReviewControllerPhase.CaughtUp
        ? state.nextDueAt
        : null
    const nextRefreshAt = Math.min(
      nextStudyDayBoundary(),
      nextLearningDueAt ?? Number.POSITIVE_INFINITY,
    )
    const delay = Math.min(
      MAX_TIMER_DELAY,
      Math.max(0, nextRefreshAt - Date.now() + CLOCK_REFRESH_GRACE),
    )
    const timer = window.setTimeout(() => {
      void controller.notifyClockChanged()
      setHomeRefreshToken((value) => value + 1)
      setClockScheduleToken((value) => value + 1)
    }, delay)
    return () => window.clearTimeout(timer)
  }, [clockScheduleToken, controller, state])

  useEffect(() => {
    const refreshClock = () => {
      if (document.visibilityState !== 'visible') {
        return
      }
      void controller.notifyClockChanged()
      setHomeRefreshToken((value) => value + 1)
      setClockScheduleToken((value) => value + 1)
    }
    window.addEventListener('focus', refreshClock)
    document.addEventListener('visibilitychange', refreshClock)

    let disposed = false
    let stopListening: (() => void) | undefined
    void listen(DaraEvent.ReviewClockRefresh, refreshClock).then((unlisten) => {
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

    void listen(DaraEvent.CardCreated, () => {
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
      const interpreted = interpretReviewKeyDown({
        altKey: event.altKey,
        canUndo: canUndo(state),
        ctrlKey: event.ctrlKey,
        isComposing: event.isComposing,
        key: event.key,
        metaKey: event.metaKey,
        phase: state.phase,
        repeat: event.repeat,
        shiftKey: event.shiftKey,
        spaceCanSubmit: spaceCanSubmit.current,
      })
      spaceCanSubmit.current = interpreted.nextSpaceCanSubmit
      if (interpreted.preventDefault) {
        event.preventDefault()
      }
      switch (interpreted.action.kind) {
        case ReviewKeyboardActionKind.None:
          break
        case ReviewKeyboardActionKind.Reveal:
          controller.reveal()
          break
        case ReviewKeyboardActionKind.MoveGradeFocus:
          controller.moveGradeFocus(interpreted.action.delta)
          break
        case ReviewKeyboardActionKind.SubmitFocusedGrade:
          void controller.submitFocusedGrade()
          break
        case ReviewKeyboardActionKind.DirectGrade:
          void controller.submitGrade(interpreted.action.grade)
          break
        case ReviewKeyboardActionKind.Undo:
          void controller.undo()
          break
      }
    }

    const handleKeyUp = (event: KeyboardEvent) => {
      spaceCanSubmit.current = interpretReviewKeyUp(
        event.key,
        spaceCanSubmit.current,
      )
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

  const showHome = useCallback(() => {
    void navigate({ to: MainWindowRoutePath.Home })
  }, [navigate])
  const showCreate = useCallback(() => {
    void navigate({ to: MainWindowRoutePath.Add })
  }, [navigate])
  const showBrowse = useCallback(() => {
    setBrowseNavigationToken((value) => value + 1)
    void navigate({ to: MainWindowRoutePath.Browse })
  }, [navigate])
  const showReview = useCallback(() => {
    if (settingsBusy) {
      return
    }
    void navigate({ to: MainWindowRoutePath.Review })
  }, [navigate, settingsBusy])
  const showSettings = useCallback(() => {
    if (settingsBusy) {
      return
    }
    setSettingsNavigationToken((value) => value + 1)
    void navigate({ to: MainWindowRoutePath.Settings })
  }, [navigate, settingsBusy])
  const refreshRestoredBackupState = useCallback(async () => {
    try {
      setRestoredBackupTakeoverAvailable(
        await loadOffsiteBackupTakeoverRequired(),
      )
    } catch {
      // Backup status errors remain actionable in Settings and must not block
      // the rest of Dara from opening.
    }
  }, [])

  useEffect(() => {
    void refreshRestoredBackupState()
    if (!restoredBackupTakeoverAvailable) {
      return
    }
    const interval = window.setInterval(() => {
      void refreshRestoredBackupState()
    }, 5_000)
    return () => window.clearInterval(interval)
  }, [refreshRestoredBackupState, restoredBackupTakeoverAvailable])
  const cancelCreate = useCallback(() => {
    void navigate({
      ignoreBlocker: true,
      replace: true,
      to: MainWindowRoutePath.Home,
    })
  }, [navigate])
  const editCard = useCallback(
    (cardContentId: string) => {
      void navigate({
        params: { cardContentId },
        to: MainWindowRoutePath.BrowseEdit,
      })
    },
    [navigate],
  )
  const selectBrowseCard = useCallback(
    (cardContentId: string, replace: boolean) => {
      void navigate({
        params: { cardContentId },
        replace,
        to: MainWindowRoutePath.BrowseCard,
      })
    },
    [navigate],
  )
  const exitEdit = useCallback(() => {
    void navigate({
      ignoreBlocker: true,
      params: editingCardContentId
        ? { cardContentId: editingCardContentId }
        : undefined,
      replace: true,
      to: editingCardContentId
        ? MainWindowRoutePath.BrowseCard
        : MainWindowRoutePath.Browse,
    })
  }, [editingCardContentId, navigate])

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
      listen(DaraEvent.OpenSettings, showSettings),
      listen(DaraEvent.OpenHome, showHome),
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
      <h1 className="visually-hidden">Dara</h1>
      <header className="main-header">
        <MainNavigation
          disabled={settingsBusy}
          onAdd={showCreate}
          onBrowse={showBrowse}
          onHome={showHome}
          onSettings={showSettings}
        />
      </header>

      {restoredBackupTakeoverAvailable && (
        <section
          aria-labelledby="restored-backup-heading"
          className="restore-takeover-banner"
          role="alert"
        >
          <div>
            <strong id="restored-backup-heading">
              This Dara was restored from an off-site backup.
            </strong>
            <span>
              New backups are paused until you confirm that this Mac should
              take over.
            </span>
          </div>
          {mode !== MainWindowMode.Settings && (
            <DaraButton onClick={showSettings} type="button">
              Review backup settings
            </DaraButton>
          )}
        </section>
      )}

      <div hidden={mode !== MainWindowMode.Home}>
        <Home
          onReview={showReview}
          refreshToken={homeRefreshToken}
        />
      </div>

      {mode === MainWindowMode.Create ? (
        <RoutedCardForm
          onCancel={cancelCreate}
          onSaved={() => {
            controller.notifyCardCreated()
            refreshHomeStats()
            cancelCreate()
          }}
        />
      ) : mode === MainWindowMode.Browse ? (
        <CardBrowser
          editingCardContentId={editingCardContentId}
          navigationToken={browseNavigationToken}
          onCardContentChanged={cardContentChanged}
          onEdit={editCard}
          onExitEdit={exitEdit}
          onQueueChanged={queueChanged}
          onSelect={selectBrowseCard}
          refreshToken={browseRefreshToken}
          selectedCardContentId={
            browseCardRouteParams
              ? browseCardRouteParams.cardContentId
              : editingCardContentId
          }
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
          <DaraButton
            className="reveal-action"
            type="button"
            onClick={() => controller.reveal()}
            variant={DaraButtonVariant.Primary}
          >
            Reveal answer
          </DaraButton>
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
          <div className="grade-grid" aria-label="Grade this card" role="group">
            {grades.map(({ grade, label }) => (
              <DaraButton
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
              </DaraButton>
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
          <DaraButton type="button" onClick={() => void controller.refresh()}>
            Refresh
          </DaraButton>
        </StatusScreen>
      )
    case ReviewControllerPhase.Error:
      return (
        <StatusScreen message="Something went wrong" detail={state.message} error>
          {state.canRetry && (
            <DaraButton type="button" onClick={() => void controller.retry()}>
              Retry
            </DaraButton>
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
