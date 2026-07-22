import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
} from 'react'
import { createPortal } from 'react-dom'
import { DaraButton } from './DaraButton.tsx'
import { DaraButtonSize, DaraButtonVariant } from './dara-button-types.ts'
import './dara-select.css'

export interface DaraSelectOption<Value extends string> {
  label: string
  value: Value
}

interface DaraSelectProps<Value extends string> {
  ariaLabel: string
  disabled?: boolean
  menuHeight?: number
  menuWidth?: number
  onReturnFocus?: () => void
  onSelect: (value: Value) => void
  options: readonly DaraSelectOption<Value>[]
  popoverClassName?: string
  tabIndex?: number
  title?: string
  triggerClassName?: string
  value: Value
}

interface MenuPosition {
  left: number
  top: number
}

const DEFAULT_MENU_WIDTH = 190
const DEFAULT_MENU_HEIGHT = 252
const MENU_GAP = 6
const VIEWPORT_MARGIN = 8

export function DaraSelect<Value extends string>({
  ariaLabel,
  disabled = false,
  menuHeight = DEFAULT_MENU_HEIGHT,
  menuWidth = DEFAULT_MENU_WIDTH,
  onReturnFocus,
  onSelect,
  options,
  popoverClassName,
  tabIndex = 0,
  title = ariaLabel,
  triggerClassName,
  value,
}: DaraSelectProps<Value>) {
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState<MenuPosition | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([])
  const menuId = useId()
  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
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
    const fitsBelow =
      bounds.bottom + MENU_GAP + menuHeight <= window.innerHeight
    setPosition({
      left: Math.max(
        VIEWPORT_MARGIN,
        Math.min(
          bounds.left,
          window.innerWidth - menuWidth - VIEWPORT_MARGIN,
        ),
      ),
      top: fitsBelow
        ? bounds.bottom + MENU_GAP
        : Math.max(VIEWPORT_MARGIN, bounds.top - menuHeight - MENU_GAP),
    })
    setOpen(true)
  }

  const closeAndReturn = () => {
    setOpen(false)
    if (onReturnFocus) {
      requestAnimationFrame(onReturnFocus)
    } else {
      requestAnimationFrame(() => triggerRef.current?.focus())
    }
  }

  const selectValue = (nextValue: Value) => {
    setOpen(false)
    onSelect(nextValue)
    if (onReturnFocus) {
      requestAnimationFrame(onReturnFocus)
    }
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
        selectValue(option.value)
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

  const currentLabel =
    options.find((option) => option.value === value)?.label ?? value
  const triggerClasses = ['dara-select-trigger', triggerClassName]
    .filter(Boolean)
    .join(' ')
  const popoverClasses = ['dara-select-popover', popoverClassName]
    .filter(Boolean)
    .join(' ')

  return (
    <>
      <DaraButton
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label={`${ariaLabel}: ${currentLabel}`}
        className={triggerClasses}
        disabled={disabled}
        onKeyDown={handleTriggerKeyDown}
        onMouseDown={handleTriggerMouseDown}
        ref={triggerRef}
        size={DaraButtonSize.Custom}
        tabIndex={tabIndex}
        title={title}
        type="button"
        variant={DaraButtonVariant.Custom}
      >
        <span>{currentLabel}</span>
        <svg
          aria-hidden="true"
          className="dara-select-chevron"
          viewBox="0 0 10 6"
        >
          <path d="M1 1 5 5 9 1" />
        </svg>
      </DaraButton>
      {open &&
        position &&
        createPortal(
          <div
            aria-label={`${ariaLabel} options`}
            role="region"
          >
            <div
              aria-label={ariaLabel}
              className={popoverClasses}
              id={menuId}
              onKeyDown={handleMenuKeyDown}
              ref={menuRef}
              role="listbox"
              style={{ ...position, width: menuWidth, maxHeight: menuHeight }}
            >
              {options.map((option, index) => {
                const selected = option.value === value
                return (
                  <DaraButton
                    aria-selected={selected}
                    className={
                      selected
                        ? 'dara-select-option dara-select-option-selected'
                        : 'dara-select-option'
                    }
                    key={option.value}
                    onClick={() => selectValue(option.value)}
                    onMouseDown={(event) => {
                      event.preventDefault()
                      selectValue(option.value)
                    }}
                    ref={(element) => {
                      optionRefs.current[index] = element
                    }}
                    role="option"
                    size={DaraButtonSize.Custom}
                    tabIndex={selected ? 0 : -1}
                    type="button"
                    variant={DaraButtonVariant.Custom}
                  >
                    <span aria-hidden="true" className="dara-select-check">
                      {selected ? '✓' : ''}
                    </span>
                    {option.label}
                  </DaraButton>
                )
              })}
            </div>
          </div>,
          document.body,
        )}
    </>
  )
}
