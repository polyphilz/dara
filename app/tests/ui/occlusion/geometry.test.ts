import { describe, expect, test } from 'vitest'
import {
  moveRect,
  normalizeRect,
  OcclusionResizeHandle,
  rectFromPoints,
  resizeRect,
} from '../../../src/occlusion/geometry.ts'

describe('normalized image-occlusion geometry', () => {
  test('creates masks in every drag direction at four-decimal precision', () => {
    expect(rectFromPoints({ x: 0.81234, y: 0.70678 }, { x: 0.11234, y: 0.20678 }))
      .toEqual({ x: 0.1123, y: 0.2068, width: 0.7, height: 0.5 })
  })

  test('clamps moves and resize handles to the natural-image bounds', () => {
    const rect = { x: 0.2, y: 0.25, width: 0.3, height: 0.2 }
    expect(moveRect(rect, { x: -0.5, y: 0.9 })).toEqual({
      x: 0,
      y: 0.8,
      width: 0.3,
      height: 0.2,
    })
    expect(
      resizeRect(rect, OcclusionResizeHandle.SouthEast, { x: 1.2, y: 1.2 }),
    ).toEqual({ x: 0.2, y: 0.25, width: 0.8, height: 0.75 })
  })

  test('serialize-parse-serialize is stable for normalized rectangles', () => {
    let seed = 0x5f3759df
    for (let index = 0; index < 500; index += 1) {
      seed = (seed * 1664525 + 1013904223) >>> 0
      const x = (seed % 9000) / 10_000
      seed = (seed * 1664525 + 1013904223) >>> 0
      const y = (seed % 9000) / 10_000
      seed = (seed * 1664525 + 1013904223) >>> 0
      const width = ((seed % Math.max(1, Math.floor((1 - x) * 10_000))) + 1) / 10_000
      seed = (seed * 1664525 + 1013904223) >>> 0
      const height = ((seed % Math.max(1, Math.floor((1 - y) * 10_000))) + 1) / 10_000
      const normalized = normalizeRect({ x, y, width, height })
      const reparsed = JSON.parse(JSON.stringify(normalized))
      expect(normalizeRect(reparsed)).toEqual(normalized)
    }
  })

})
