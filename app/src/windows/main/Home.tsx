import { ActivityCalendar, type Activity } from 'react-activity-calendar'
import { memo, useEffect, useMemo, useState } from 'react'
import { DaraButton } from '../../components/DaraButton.tsx'
import { DaraButtonSize } from '../../components/dara-button-types.ts'
import { DaraText } from '../../components/DaraText.tsx'
import {
  DaraTextTone,
  DaraTextVariant,
} from '../../components/dara-text-types.ts'
import { errorMessage } from '../../review/errors.ts'
import type { HomeStats, LoadHomeStatsInput } from '../../review/index.ts'
import { captureStudyMoment } from '../../scheduling/index.ts'
import { Appearance } from '../../settings/index.ts'
import {
  buildActivityCalendarData,
  resolveActivityColorScheme,
} from './home-activity.ts'
import {
  getHomeStats,
  invalidateHomeStats,
  peekHomeStats,
} from './home-stats-cache.ts'

const ACTIVITY_DAY_COUNT = 365
const ACTIVITY_GRAPH_SCALE = 720 / 739
const MAX_TIMER_DELAY = 2_147_000_000
const SYSTEM_DARK_COLOR_SCHEME_QUERY = '(prefers-color-scheme: dark)'
const ACTIVITY_THEME = {
  light: ['#e8e3dc', '#f6d6b0', '#f4c385', '#f1ae5b', '#ee9c40'],
  dark: ['#302d29', '#6c4a2b', '#9a612a', '#c77c32', '#ee9c40'],
}

interface HomeProps {
  onReview: () => void
  refreshToken?: number
}

export const Home = memo(function Home({
  onReview,
  refreshToken = 0,
}: HomeProps) {
  const [request, setRequest] = useState<LoadHomeStatsInput>(createHomeRequest)
  const [stats, setStats] = useState<HomeStats | null>(() =>
    peekHomeStats(request),
  )
  const [error, setError] = useState<string | null>(null)
  const [reloadToken, setReloadToken] = useState(0)
  const calendarColorScheme = useActivityColorScheme()

  useEffect(() => {
    let disposed = false
    const nextRequest = createHomeRequest()
    const existing = peekHomeStats(nextRequest)
    setRequest(nextRequest)
    setError(null)
    if (existing) {
      setStats(existing)
      return
    }

    setStats(null)
    void getHomeStats(nextRequest)
      .then((result) => {
        if (!disposed) {
          setStats(result)
        }
      })
      .catch((loadError: unknown) => {
        if (!disposed) {
          setError(errorMessage(loadError))
        }
      })

    return () => {
      disposed = true
    }
  }, [refreshToken, reloadToken])

  useEffect(() => {
    if (stats === null || stats.nextLearningDueAt === null) {
      return
    }
    const delay = Math.min(
      MAX_TIMER_DELAY,
      Math.max(0, stats.nextLearningDueAt - Date.now() + 25),
    )
    const timer = window.setTimeout(
      () => setReloadToken((value) => value + 1),
      delay,
    )
    return () => window.clearTimeout(timer)
  }, [reloadToken, stats])

  const activity = useMemo(
    () =>
      stats
        ? buildActivityCalendarData(
            stats.activity,
            request.activityStartStudyDay,
            request.studyDay,
          )
        : null,
    [request, stats],
  )

  return (
    <section className="home-screen">
      <section aria-label="Review activity" className="home-activity">
        {activity ? (
          <ActivityCalendar
            blockMargin={3 * ACTIVITY_GRAPH_SCALE}
            blockRadius={3 * ACTIVITY_GRAPH_SCALE}
            blockSize={11 * ACTIVITY_GRAPH_SCALE}
            colorScheme={calendarColorScheme}
            data={activity}
            /*
             * Dynamic SVG label geometry: the calendar's label size must scale
             * with the block size, so it stays computed rather than tokenized.
             */
            fontSize={12 * ACTIVITY_GRAPH_SCALE}
            labels={{
              legend: { less: 'Less', more: 'More' },
            }}
            showTotalCount={false}
            showWeekdayLabels={['mon', 'wed', 'fri']}
            theme={ACTIVITY_THEME}
            tooltips={{
              activity: {
                text: activityTooltip,
                withArrow: true,
              },
            }}
          />
        ) : error ? (
          <DaraText
            aria-live="polite"
            as="div"
            className="home-activity-loading"
            tone={DaraTextTone.Muted}
            variant={DaraTextVariant.Supporting}
          >
            Activity unavailable.
          </DaraText>
        ) : (
          <DaraText
            aria-live="polite"
            as="div"
            className="home-activity-loading"
            tone={DaraTextTone.Muted}
            variant={DaraTextVariant.Supporting}
          >
            Loading activity…
          </DaraText>
        )}
      </section>

      {error && (
        <div className="home-error" role="alert">
          <DaraText
            as="span"
            tone={DaraTextTone.Danger}
            variant={DaraTextVariant.Supporting}
          >
            {error}
          </DaraText>
          <DaraButton
            onClick={() => {
              invalidateHomeStats()
              setReloadToken((value) => value + 1)
            }}
            type="button"
          >
            Try again
          </DaraButton>
        </div>
      )}
      <DaraButton
        className="home-review-card"
        onClick={onReview}
        size={DaraButtonSize.Custom}
        type="button"
      >
        <span className="home-review-card-heading">
          <span>Review</span>
          <span aria-hidden="true">→</span>
        </span>
        <span className="home-reviewed-today">
          <strong>{stats?.reviewedToday ?? '—'}</strong>
          <span>reviewed today</span>
        </span>
        <span className="home-queue-counts">
          <QueueCount label="New" value={stats?.queue.new} />
          <QueueCount label="Learning" value={stats?.queue.learning} />
          <QueueCount label="Review" value={stats?.queue.review} />
        </span>
      </DaraButton>
    </section>
  )
})

function useActivityColorScheme() {
  const [colorScheme, setColorScheme] = useState(readActivityColorScheme)

  useEffect(() => {
    const systemScheme = window.matchMedia(SYSTEM_DARK_COLOR_SCHEME_QUERY)
    const refresh = () => setColorScheme(readActivityColorScheme())
    const appearanceObserver = new MutationObserver(refresh)
    appearanceObserver.observe(document.documentElement, {
      attributeFilter: ['data-appearance'],
      attributes: true,
    })
    systemScheme.addEventListener('change', refresh)
    refresh()
    return () => {
      appearanceObserver.disconnect()
      systemScheme.removeEventListener('change', refresh)
    }
  }, [])

  return colorScheme
}

function readActivityColorScheme() {
  const storedAppearance = document.documentElement.dataset.appearance
  const appearance =
    storedAppearance === Appearance.Dark
      ? Appearance.Dark
      : storedAppearance === Appearance.Light
        ? Appearance.Light
        : Appearance.System
  return resolveActivityColorScheme(
    appearance,
    window.matchMedia(SYSTEM_DARK_COLOR_SCHEME_QUERY).matches,
  )
}

function createHomeRequest(): LoadHomeStatsInput {
  const moment = captureStudyMoment()
  return {
    now: moment.reviewedAt,
    studyDay: moment.studyDay,
    activityStartStudyDay: moment.studyDay - (ACTIVITY_DAY_COUNT - 1),
  }
}

function QueueCount({ label, value }: { label: string; value?: number }) {
  return (
    <span>
      <strong>{value ?? '—'}</strong>
      <span>{label}</span>
    </span>
  )
}

function activityTooltip(activity: Activity): string {
  const reviews = activity.count === 1 ? 'review' : 'reviews'
  const date = new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeZone: 'UTC',
  }).format(new Date(`${activity.date}T00:00:00Z`))
  return `${activity.count} ${reviews} on ${date}`
}
