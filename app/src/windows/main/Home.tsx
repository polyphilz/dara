import { ActivityCalendar, type Activity } from 'react-activity-calendar'
import { memo, useEffect, useMemo, useState } from 'react'
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
      <section aria-labelledby="review-activity-title" className="home-activity">
        <div className="home-section-heading">
          <h1 id="review-activity-title">Review activity</h1>
        </div>
        {activity ? (
          <ActivityCalendar
            blockMargin={3}
            blockRadius={3}
            blockSize={10}
            colorScheme={calendarColorScheme}
            data={activity}
            fontSize={11}
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
          <div className="home-activity-loading" aria-live="polite">
            Activity unavailable.
          </div>
        ) : (
          <div className="home-activity-loading" aria-live="polite">
            Loading activity…
          </div>
        )}
      </section>

      {error && (
        <div className="home-error" role="alert">
          <span>{error}</span>
          <button
            onClick={() => {
              invalidateHomeStats()
              setReloadToken((value) => value + 1)
            }}
            type="button"
          >
            Try again
          </button>
        </div>
      )}
      <button className="home-review-card" onClick={onReview} type="button">
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
      </button>
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
