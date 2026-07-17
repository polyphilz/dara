import { fireEvent, render } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { DaraShortcutRecorder } from '../../../src/components/DaraShortcutRecorder.tsx'
import {
  acceleratorForKeyboardEvent,
  formatAccelerator,
} from '../../../src/components/shortcut-accelerator.ts'

test('formats Dara accelerators using macOS shortcut symbols', () => {
  expect(formatAccelerator('control+alt+super+KeyD')).toBe('⌃⌥⌘D')
  expect(formatAccelerator('shift+super+Digit4')).toBe('⇧⌘4')
})

test('requires a modifier and rejects unsupported media keys', () => {
  expect(
    acceleratorForKeyboardEvent(keyboardEvent('KeyD')),
  ).toEqual({ message: 'Include at least one modifier key.' })
  expect(
    acceleratorForKeyboardEvent(
      keyboardEvent('MediaPlayPause', { metaKey: true }),
    ),
  ).toEqual({ message: 'That key cannot be used as a global shortcut.' })
})

test('captures a real chord only after entering recording mode', () => {
  const onCapture = vi.fn()
  const { getByRole } = render(
    <DaraShortcutRecorder
      accelerator="control+alt+super+KeyD"
      label="Quick Add shortcut"
      onCapture={onCapture}
    />,
  )
  const button = getByRole('button', { name: /Quick Add shortcut/ })

  fireEvent.keyDown(button, { code: 'KeyQ', metaKey: true })
  expect(onCapture).not.toHaveBeenCalled()
  fireEvent.click(button)
  fireEvent.keyDown(button, { code: 'KeyQ', metaKey: true })

  expect(onCapture).toHaveBeenCalledWith('super+KeyQ')
})

function keyboardEvent(
  code: string,
  overrides: Partial<{
    altKey: boolean
    ctrlKey: boolean
    metaKey: boolean
    shiftKey: boolean
  }> = {},
) {
  return {
    altKey: false,
    code,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...overrides,
  }
}
