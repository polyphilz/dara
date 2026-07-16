import { expect, test } from 'vitest'
import {
  AppZoomCommand,
  MAX_ZOOM_PERCENT,
  MIN_ZOOM_PERCENT,
  zoomCommandForKeyboardEvent,
  zoomPercentForCommand,
} from '../../../src/zoom/app-zoom.ts'

test('moves in ten-percent steps and clamps to sane bounds', () => {
  expect(zoomPercentForCommand(100, AppZoomCommand.ZoomIn)).toBe(110)
  expect(zoomPercentForCommand(100, AppZoomCommand.ZoomOut)).toBe(90)
  expect(zoomPercentForCommand(170, AppZoomCommand.Reset)).toBe(100)
  expect(
    zoomPercentForCommand(MAX_ZOOM_PERCENT, AppZoomCommand.ZoomIn),
  ).toBe(MAX_ZOOM_PERCENT)
  expect(
    zoomPercentForCommand(MIN_ZOOM_PERCENT, AppZoomCommand.ZoomOut),
  ).toBe(MIN_ZOOM_PERCENT)
})

test('maps macOS zoom shortcuts from the keyboard row and numpad', () => {
  expect(commandFor('Equal')).toBe(AppZoomCommand.ZoomIn)
  expect(commandFor('NumpadAdd')).toBe(AppZoomCommand.ZoomIn)
  expect(commandFor('Minus')).toBe(AppZoomCommand.ZoomOut)
  expect(commandFor('NumpadSubtract')).toBe(AppZoomCommand.ZoomOut)
  expect(commandFor('Digit0')).toBe(AppZoomCommand.Reset)
  expect(commandFor('Numpad0')).toBe(AppZoomCommand.Reset)
})

test('leaves modified and unrelated shortcuts alone', () => {
  expect(commandFor('Equal', { metaKey: false })).toBeNull()
  expect(commandFor('Equal', { altKey: true })).toBeNull()
  expect(commandFor('Equal', { ctrlKey: true })).toBeNull()
  expect(commandFor('KeyA')).toBeNull()
})

function commandFor(
  code: string,
  overrides: Partial<{
    altKey: boolean
    ctrlKey: boolean
    metaKey: boolean
  }> = {},
) {
  return zoomCommandForKeyboardEvent({
    altKey: false,
    code,
    ctrlKey: false,
    metaKey: true,
    ...overrides,
  })
}
