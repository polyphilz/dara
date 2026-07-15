import {
  loadHomeStats,
  type HomeStats,
  type LoadHomeStatsInput,
} from '../../review/index.ts'

type HomeStatsLoader = (input: LoadHomeStatsInput) => Promise<HomeStats>

interface CachedHomeStats {
  key: string
  stats: HomeStats
}

interface PendingHomeStats {
  key: string
  version: number
  promise: Promise<HomeStats>
}

export interface HomeStatsCache {
  get: (input: LoadHomeStatsInput) => Promise<HomeStats>
  invalidate: () => void
  peek: (input: LoadHomeStatsInput) => HomeStats | null
}

export function createHomeStatsCache(loader: HomeStatsLoader): HomeStatsCache {
  let cached: CachedHomeStats | null = null
  let pending: PendingHomeStats | null = null
  let version = 0
  let requestSequence = 0

  const peek = (input: LoadHomeStatsInput): HomeStats | null => {
    if (!cached || cached.key !== cacheKey(input)) {
      return null
    }
    return isCurrent(cached.stats, input) ? cached.stats : null
  }

  const get = (input: LoadHomeStatsInput): Promise<HomeStats> => {
    const existing = peek(input)
    if (existing) {
      return Promise.resolve(existing)
    }

    const key = cacheKey(input)
    if (pending && pending.key === key && pending.version === version) {
      const pendingLoad = pending
      return pendingLoad.promise.then((stats) => {
        if (version === pendingLoad.version && isCurrent(stats, input)) {
          return stats
        }
        if (pending?.promise === pendingLoad.promise) {
          pending = null
        }
        return get(input)
      })
    }

    const requestVersion = version
    const sequence = ++requestSequence
    const promise = loader(input).then((stats) => {
      if (version === requestVersion && requestSequence === sequence) {
        cached = { key, stats }
      }
      return stats
    })
    pending = { key, version: requestVersion, promise }
    const clearPending = () => {
      if (pending?.promise === promise) {
        pending = null
      }
    }
    void promise.then(clearPending, clearPending)
    return promise
  }

  const invalidate = () => {
    version += 1
    requestSequence += 1
    cached = null
    pending = null
  }

  return { get, invalidate, peek }
}

function cacheKey(input: LoadHomeStatsInput): string {
  return `${input.activityStartStudyDay}:${input.studyDay}`
}

function isCurrent(stats: HomeStats, input: LoadHomeStatsInput): boolean {
  return (
    stats.nextLearningDueAt === null ||
    input.now < stats.nextLearningDueAt
  )
}

const homeStatsCache = createHomeStatsCache(loadHomeStats)

export const getHomeStats = homeStatsCache.get
export const invalidateHomeStats = homeStatsCache.invalidate
export const peekHomeStats = homeStatsCache.peek
