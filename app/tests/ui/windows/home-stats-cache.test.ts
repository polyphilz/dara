import { expect, test, vi } from 'vitest'
import type {
  HomeStats,
  LoadHomeStatsInput,
} from '../../../src/review/contracts.ts'
import { createHomeStatsCache } from '../../../src/windows/main/home-stats-cache.ts'

const request: LoadHomeStatsInput = {
  now: 1_000,
  studyDay: 20,
  activityStartStudyDay: 10,
}

const stats: HomeStats = {
  activity: [],
  reviewedToday: 4,
  queue: { new: 1, learning: 2, review: 3 },
  nextLearningDueAt: 2_000,
}

test('reuses stats until their next learning deadline', async () => {
  const loader = vi.fn().mockResolvedValue(stats)
  const cache = createHomeStatsCache(loader)

  await expect(cache.get(request)).resolves.toBe(stats)
  await expect(cache.get({ ...request, now: 1_999 })).resolves.toBe(stats)

  expect(loader).toHaveBeenCalledTimes(1)

  await cache.get({ ...request, now: 2_000 })

  expect(loader).toHaveBeenCalledTimes(2)
})

test('invalidates on mutations and study-day rollover', async () => {
  const loader = vi.fn().mockResolvedValue(stats)
  const cache = createHomeStatsCache(loader)

  await cache.get(request)
  cache.invalidate()
  await cache.get(request)
  await cache.get({
    ...request,
    studyDay: request.studyDay + 1,
    activityStartStudyDay: request.activityStartStudyDay + 1,
  })

  expect(loader).toHaveBeenCalledTimes(3)
})

test('deduplicates concurrent loads for the same snapshot', async () => {
  let resolveLoad: ((value: HomeStats) => void) | undefined
  const loader = vi.fn(
    () =>
      new Promise<HomeStats>((resolve) => {
        resolveLoad = resolve
      }),
  )
  const cache = createHomeStatsCache(loader)

  const first = cache.get(request)
  const second = cache.get(request)
  resolveLoad?.(stats)

  await expect(first).resolves.toBe(stats)
  await expect(second).resolves.toBe(stats)
  expect(loader).toHaveBeenCalledTimes(1)
})
