import { describe, expect, test } from 'vitest'
import fc from 'fast-check'
import {
  MinimumOcclusionMaskSize,
  OcclusionResizeHandle,
  moveRect,
  normalizeRect,
  rectFromPoints,
  resizeRect,
  type NormalizedRect,
} from '../../../src/occlusion/geometry.ts'

const PROPERTY_SEED = Number(process.env.DARA_PROPERTY_SEED ?? Date.now())
const PROPERTY_RUNS = Number(
  process.env.DARA_PROPERTY_RUNS ??
    (process.env.DARA_PROPERTY_SEED ? 250 : 2_000),
)

const finiteCoordinate = fc.double({
  min: -2,
  max: 2,
  noDefaultInfinity: true,
  noNaN: true,
})
const point = fc.record({ x: finiteCoordinate, y: finiteCoordinate })
const rect = fc.record({
  height: finiteCoordinate,
  width: finiteCoordinate,
  x: finiteCoordinate,
  y: finiteCoordinate,
})
const handle = fc.constantFrom(...Object.values(OcclusionResizeHandle))

function expectCanonicalRect(value: NormalizedRect) {
  expect(value.x).toBeGreaterThanOrEqual(0)
  expect(value.y).toBeGreaterThanOrEqual(0)
  expect(value.width).toBeGreaterThanOrEqual(MinimumOcclusionMaskSize)
  expect(value.height).toBeGreaterThanOrEqual(MinimumOcclusionMaskSize)
  expect(value.x + value.width).toBeLessThanOrEqual(1)
  expect(value.y + value.height).toBeLessThanOrEqual(1)
  for (const coordinate of Object.values(value)) {
    expect(Number.isFinite(coordinate)).toBe(true)
    expect(coordinate).toBe(Number(coordinate.toFixed(4)))
  }
}

describe('occlusion geometry properties', () => {
  test('normalize, move, and resize always stay in normalized bounds', () => {
    fc.assert(
      fc.property(rect, point, point, handle, (candidate, delta, target, resizeHandle) => {
        const normalized = normalizeRect(candidate)
        expectCanonicalRect(normalized)
        expectCanonicalRect(moveRect(normalized, delta))
        expectCanonicalRect(resizeRect(normalized, resizeHandle, target))
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })

  test('drag direction does not change the canonical rectangle', () => {
    fc.assert(
      fc.property(point, point, (start, end) => {
        expect(rectFromPoints(start, end)).toEqual(rectFromPoints(end, start))
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })

  test('four-decimal JSON round trips remain stable', () => {
    fc.assert(
      fc.property(rect, (candidate) => {
        const normalized = normalizeRect(candidate)
        const parsed = JSON.parse(JSON.stringify(normalized)) as NormalizedRect
        expect(normalizeRect(parsed)).toEqual(normalized)
      }),
      { numRuns: PROPERTY_RUNS, seed: PROPERTY_SEED },
    )
  })
})
