import { useLayoutEffect, useRef, useState } from 'react'
import { DaraButton } from '../../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../components/dara-button-types.ts'
import { RichTextEditor } from '../../markdown/RichTextEditor.tsx'
import { RichTextToolbarControl } from '../../markdown/rich-text-toolbar-controls.ts'

const SCRATCHPAD_HIDDEN_TOOLBAR_CONTROLS = [
  RichTextToolbarControl.Bold,
  RichTextToolbarControl.Italic,
  RichTextToolbarControl.Strikethrough,
  RichTextToolbarControl.Link,
  RichTextToolbarControl.BlockQuote,
] as const

const SCRATCHPAD_VIEWPORT_BOTTOM_PADDING = 48

interface ReviewScratchpadProps {
  hidden: boolean
  id: string
  sessionKey: string
}

interface ReviewScratchpadToggleProps {
  controls: string
  open: boolean
  onToggle: () => void
}

interface ScratchpadDraft {
  sessionKey: string
  value: string
}

export function ReviewScratchpad({
  hidden,
  id,
  sessionKey,
}: ReviewScratchpadProps) {
  const [draft, setDraft] = useState<ScratchpadDraft>(() => ({
    sessionKey,
    value: '',
  }))
  const scratchpadRef = useRef<HTMLElement>(null)
  const value = draft.sessionKey === sessionKey ? draft.value : ''
  if (draft.sessionKey !== sessionKey) {
    setDraft({ sessionKey, value: '' })
  }

  useLayoutEffect(() => {
    const scratchpad = scratchpadRef.current
    if (hidden || !scratchpad) {
      return
    }

    const viewport = window.visualViewport
    let animationFrame: number | null = null
    const updateMaximumHeight = () => {
      animationFrame = null
      const viewportBottom = viewport
        ? viewport.offsetTop + viewport.height
        : window.innerHeight
      const availableHeight = Math.max(
        0,
        viewportBottom -
          scratchpad.getBoundingClientRect().top -
          SCRATCHPAD_VIEWPORT_BOTTOM_PADDING,
      )
      scratchpad.style.setProperty(
        '--review-scratchpad-max-height',
        `${availableHeight}px`,
      )
    }
    const scheduleMaximumHeightUpdate = () => {
      if (animationFrame === null) {
        animationFrame = window.requestAnimationFrame(updateMaximumHeight)
      }
    }

    updateMaximumHeight()
    window.addEventListener('resize', scheduleMaximumHeightUpdate)
    window.addEventListener('scroll', scheduleMaximumHeightUpdate, {
      passive: true,
    })
    viewport?.addEventListener('resize', scheduleMaximumHeightUpdate)
    viewport?.addEventListener('scroll', scheduleMaximumHeightUpdate)
    return () => {
      if (animationFrame !== null) {
        window.cancelAnimationFrame(animationFrame)
      }
      window.removeEventListener('resize', scheduleMaximumHeightUpdate)
      window.removeEventListener('scroll', scheduleMaximumHeightUpdate)
      viewport?.removeEventListener('resize', scheduleMaximumHeightUpdate)
      viewport?.removeEventListener('scroll', scheduleMaximumHeightUpdate)
    }
  }, [hidden, sessionKey])

  return (
    <section
      aria-label="Scratchpad"
      className="review-scratchpad"
      hidden={hidden}
      id={id}
      ref={scratchpadRef}
    >
      <RichTextEditor
        ariaLabel="Scratchpad"
        hiddenToolbarControls={SCRATCHPAD_HIDDEN_TOOLBAR_CONTROLS}
        onChange={(nextValue) => setDraft({ sessionKey, value: nextValue })}
        placeholder="Work out your answer…"
        resetKey={sessionKey}
        value={value}
      />
    </section>
  )
}

export function ReviewScratchpadToggle({
  controls,
  open,
  onToggle,
}: ReviewScratchpadToggleProps) {
  const label = open ? 'Hide scratchpad' : 'Open a scratchpad'
  return (
    <DaraButton
      aria-controls={controls}
      aria-expanded={open}
      aria-label={label}
      className="review-scratchpad-toggle"
      onClick={onToggle}
      size={DaraButtonSize.Icon}
      title={label}
      type="button"
      variant={DaraButtonVariant.Ghost}
    >
      <PencilIcon />
    </DaraButton>
  )
}

function PencilIcon() {
  return (
    <svg
      aria-hidden="true"
      className="review-scratchpad-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <path d="M4 20l1.1-4.2L16 4.9a2.1 2.1 0 0 1 3 3L8.2 18.9 4 20Z" />
      <path d="m14.5 6.5 3 3" />
    </svg>
  )
}
