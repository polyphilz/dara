export const OcclusionResizeHandle = {
  NorthWest: 'NORTH_WEST',
  North: 'NORTH',
  NorthEast: 'NORTH_EAST',
  East: 'EAST',
  SouthEast: 'SOUTH_EAST',
  South: 'SOUTH',
  SouthWest: 'SOUTH_WEST',
  West: 'WEST',
} as const

export type OcclusionResizeHandle =
  (typeof OcclusionResizeHandle)[keyof typeof OcclusionResizeHandle]

export interface NormalizedPoint {
  x: number
  y: number
}

export interface NormalizedRect {
  x: number
  y: number
  width: number
  height: number
}

export const OcclusionCoordinatePrecision = 4
export const MinimumOcclusionMaskSize = 0.0025

const coordinateScale = 10 ** OcclusionCoordinatePrecision

export function normalizedPoint(
  clientX: number,
  clientY: number,
  bounds: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>,
): NormalizedPoint {
  return {
    x: quantize(clamp((clientX - bounds.left) / bounds.width, 0, 1)),
    y: quantize(clamp((clientY - bounds.top) / bounds.height, 0, 1)),
  }
}

export function rectFromPoints(
  start: NormalizedPoint,
  end: NormalizedPoint,
): NormalizedRect | null {
  const rect = normalizeRect({
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  })
  return rect.width >= MinimumOcclusionMaskSize &&
    rect.height >= MinimumOcclusionMaskSize
    ? rect
    : null
}

export function moveRect(
  rect: NormalizedRect,
  delta: NormalizedPoint,
): NormalizedRect {
  return normalizeRect({
    ...rect,
    x: clamp(rect.x + delta.x, 0, 1 - rect.width),
    y: clamp(rect.y + delta.y, 0, 1 - rect.height),
  })
}

export function resizeRect(
  rect: NormalizedRect,
  handle: OcclusionResizeHandle,
  point: NormalizedPoint,
): NormalizedRect {
  let left = rect.x
  let top = rect.y
  let right = rect.x + rect.width
  let bottom = rect.y + rect.height

  if (westHandles.has(handle)) {
    left = clamp(point.x, 0, right - MinimumOcclusionMaskSize)
  }
  if (eastHandles.has(handle)) {
    right = clamp(point.x, left + MinimumOcclusionMaskSize, 1)
  }
  if (northHandles.has(handle)) {
    top = clamp(point.y, 0, bottom - MinimumOcclusionMaskSize)
  }
  if (southHandles.has(handle)) {
    bottom = clamp(point.y, top + MinimumOcclusionMaskSize, 1)
  }

  return normalizeRect({
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  })
}

export function normalizeRect(rect: NormalizedRect): NormalizedRect {
  const x = quantize(clamp(rect.x, 0, 1 - MinimumOcclusionMaskSize))
  const y = quantize(clamp(rect.y, 0, 1 - MinimumOcclusionMaskSize))
  const width = quantize(
    clamp(rect.width, MinimumOcclusionMaskSize, 1 - x),
  )
  const height = quantize(
    clamp(rect.height, MinimumOcclusionMaskSize, 1 - y),
  )
  return {
    x,
    y,
    width: quantize(Math.min(width, 1 - x)),
    height: quantize(Math.min(height, 1 - y)),
  }
}

export function rectEquals(a: NormalizedRect, b: NormalizedRect): boolean {
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height
}

function quantize(value: number): number {
  return Math.round(value * coordinateScale) / coordinateScale
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value))
}

const westHandles = new Set<OcclusionResizeHandle>([
  OcclusionResizeHandle.NorthWest,
  OcclusionResizeHandle.SouthWest,
  OcclusionResizeHandle.West,
])
const eastHandles = new Set<OcclusionResizeHandle>([
  OcclusionResizeHandle.NorthEast,
  OcclusionResizeHandle.SouthEast,
  OcclusionResizeHandle.East,
])
const northHandles = new Set<OcclusionResizeHandle>([
  OcclusionResizeHandle.NorthWest,
  OcclusionResizeHandle.North,
  OcclusionResizeHandle.NorthEast,
])
const southHandles = new Set<OcclusionResizeHandle>([
  OcclusionResizeHandle.SouthWest,
  OcclusionResizeHandle.South,
  OcclusionResizeHandle.SouthEast,
])
