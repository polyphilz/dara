import { useEffect, useRef, useState } from 'react'
import {
  acceleratorForKeyboardEvent,
  formatAccelerator,
} from './shortcut-accelerator.ts'

interface DaraShortcutRecorderProps {
  accelerator: string
  disabled?: boolean
  label: string
  onCapture: (accelerator: string) => void
  resetToken?: number
}

const modifierCodes = new Set([
  'AltLeft',
  'AltRight',
  'ControlLeft',
  'ControlRight',
  'MetaLeft',
  'MetaRight',
  'ShiftLeft',
  'ShiftRight',
])

export function DaraShortcutRecorder({
  accelerator,
  disabled = false,
  label,
  onCapture,
  resetToken = 0,
}: DaraShortcutRecorderProps) {
  const [recording, setRecording] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!recording) {
      return
    }
    const capture = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopImmediatePropagation()
        setRecording(false)
        setError(null)
        requestAnimationFrame(() => buttonRef.current?.focus())
        return
      }
      if (modifierCodes.has(event.code)) {
        return
      }
      event.preventDefault()
      event.stopImmediatePropagation()
      const result = acceleratorForKeyboardEvent(event)
      if (typeof result !== 'string') {
        setError(result.message)
        return
      }
      setRecording(false)
      setError(null)
      onCapture(result)
    }
    window.addEventListener('keydown', capture, { capture: true })
    return () => window.removeEventListener('keydown', capture, { capture: true })
  }, [onCapture, recording])

  useEffect(() => {
    setRecording(false)
    setError(null)
  }, [resetToken])

  return (
    <div className="shortcut-recorder">
      <button
        aria-describedby={error ? `${label}-shortcut-error` : undefined}
        aria-label={`${label}: ${formatAccelerator(accelerator)}`}
        className={recording ? 'shortcut-recorder-button recording' : 'shortcut-recorder-button'}
        disabled={disabled}
        onBlur={() => {
          if (recording) {
            setRecording(false)
            setError(null)
          }
        }}
        onClick={() => {
          setRecording(true)
          setError(null)
        }}
        ref={buttonRef}
        type="button"
      >
        {recording ? 'Press shortcut…' : formatAccelerator(accelerator)}
      </button>
      {error && (
        <span className="shortcut-recorder-error" id={`${label}-shortcut-error`} role="alert">
          {error}
        </span>
      )}
      {recording && !error && (
        <span className="shortcut-recorder-hint">Press a complete shortcut · Esc to cancel</span>
      )}
    </div>
  )
}
