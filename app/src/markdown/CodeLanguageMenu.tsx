import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
} from 'react'
import { createPortal } from 'react-dom'
import {
  codeLanguageDefinitions,
  codeLanguageDisplayName,
} from './languages.ts'

interface CodeLanguageMenuProps {
  disabled: boolean
  language: string | null
  onReturnFocus: () => void
  onSelect: (language: string | null) => void
}

interface LanguageOption {
  label: string
  value: string
}

interface MenuPosition {
  left: number
  top: number
}

const menuWidth = 190
const menuHeight = 252
const viewportMargin = 8

export function CodeLanguageMenu({
  disabled,
  language,
  onReturnFocus,
  onSelect,
}: CodeLanguageMenuProps) {
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState<MenuPosition | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([])
  const menuId = useId()
  const options = languageOptions(language)
  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === (language ?? '')),
  )

  useEffect(() => {
    if (!open) {
      return
    }

    const focusFrame = requestAnimationFrame(() => {
      optionRefs.current[selectedIndex]?.focus()
    })
    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target
      if (
        target instanceof Node &&
        !triggerRef.current?.contains(target) &&
        !menuRef.current?.contains(target)
      ) {
        setOpen(false)
      }
    }
    document.addEventListener('pointerdown', closeOnOutsidePointer, true)
    return () => {
      cancelAnimationFrame(focusFrame)
      document.removeEventListener('pointerdown', closeOnOutsidePointer, true)
    }
  }, [open, selectedIndex])

  const showMenu = () => {
    const trigger = triggerRef.current
    if (!trigger) {
      return
    }
    const bounds = trigger.getBoundingClientRect()
    const fitsBelow = bounds.bottom + 6 + menuHeight <= window.innerHeight
    setPosition({
      left: Math.max(
        viewportMargin,
        Math.min(bounds.left, window.innerWidth - menuWidth - viewportMargin),
      ),
      top: fitsBelow
        ? bounds.bottom + 6
        : Math.max(viewportMargin, bounds.top - menuHeight - 6),
    })
    setOpen(true)
  }

  const closeAndReturn = () => {
    setOpen(false)
    requestAnimationFrame(onReturnFocus)
  }

  const selectLanguage = (value: string) => {
    setOpen(false)
    onSelect(value || null)
  }

  const handleTriggerMouseDown = (event: MouseEvent<HTMLButtonElement>) => {
    event.preventDefault()
    if (open) {
      closeAndReturn()
    } else {
      showMenu()
    }
  }

  const handleTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'Escape' && open) {
      event.preventDefault()
      event.stopPropagation()
      closeAndReturn()
      return
    }
    if (![' ', 'Enter', 'ArrowDown', 'ArrowUp'].includes(event.key)) {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    if (!open) {
      showMenu()
    }
  }

  const handleMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const current = optionRefs.current.indexOf(
      document.activeElement as HTMLButtonElement,
    )
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      closeAndReturn()
      return
    }
    if (event.key === 'Tab') {
      setOpen(false)
      return
    }
    if (event.key === 'Enter' || event.key === ' ') {
      const option = options[current]
      if (option) {
        event.preventDefault()
        event.stopPropagation()
        selectLanguage(option.value)
      }
      return
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? options.length - 1
          : event.key === 'ArrowDown'
            ? (current + 1 + options.length) % options.length
            : (current - 1 + options.length) % options.length
    optionRefs.current[next]?.focus()
  }

  const currentLabel = codeLanguageDisplayName(language)
  return (
    <>
      <button
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label={`Code language: ${currentLabel}`}
        className="toolbar-button code-language-trigger"
        disabled={disabled}
        onKeyDown={handleTriggerKeyDown}
        onMouseDown={handleTriggerMouseDown}
        ref={triggerRef}
        tabIndex={-1}
        title="Code language"
        type="button"
      >
        <span>{currentLabel}</span>
        <span aria-hidden="true" className="code-language-chevron">⌄</span>
      </button>
      {open && position &&
        createPortal(
          <div
            aria-label="Code language"
            className="code-language-popover"
            id={menuId}
            onKeyDown={handleMenuKeyDown}
            ref={menuRef}
            role="listbox"
            style={position}
          >
            {options.map((option, index) => {
              const selected = option.value === (language ?? '')
              return (
                <button
                  aria-selected={selected}
                  className={
                    selected
                      ? 'code-language-option code-language-option-selected'
                      : 'code-language-option'
                  }
                  key={option.value || 'plain'}
                  onClick={() => selectLanguage(option.value)}
                  onMouseDown={(event) => {
                    event.preventDefault()
                    selectLanguage(option.value)
                  }}
                  ref={(element) => {
                    optionRefs.current[index] = element
                  }}
                  role="option"
                  tabIndex={selected ? 0 : -1}
                  type="button"
                >
                  <span aria-hidden="true" className="code-language-check">
                    {selected ? '✓' : ''}
                  </span>
                  {option.label}
                </button>
              )
            })}
          </div>,
          document.body,
        )}
    </>
  )
}

function languageOptions(language: string | null): LanguageOption[] {
  const known = codeLanguageDefinitions.some(
    (definition) => definition.canonical === language,
  )
  return [
    { label: 'Plain code', value: '' },
    ...(language && !known
      ? [{ label: codeLanguageDisplayName(language), value: language }]
      : []),
    ...codeLanguageDefinitions.map((definition) => ({
      label: codeLanguageDisplayName(definition.canonical),
      value: definition.canonical,
    })),
  ]
}
