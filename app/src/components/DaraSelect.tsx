import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react'
import { createPortal } from 'react-dom'
import { DaraButton } from './DaraButton.tsx'
import { DARA_WRITING_ASSISTANCE_PROPS } from './writing-assistance.ts'
import { DaraButtonSize, DaraButtonVariant } from './dara-button-types.ts'
import './dara-select.css'

export interface DaraSelectOption<Value extends string> {
  /** Extra terms the search box should match, such as language aliases. */
  keywords?: readonly string[]
  label: string
  value: Value
}

interface DaraSelectProps<Value extends string> {
  ariaLabel: string
  disabled?: boolean
  id?: string
  menuHeight?: number
  menuWidth?: number
  onReturnFocus?: () => void
  onSelect: (value: Value) => void
  options: readonly DaraSelectOption<Value>[]
  popoverClassName?: string
  /** Opt-in filter box above the options; other selects are unaffected. */
  searchable?: boolean
  searchPlaceholder?: string
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
  id,
  menuHeight = DEFAULT_MENU_HEIGHT,
  menuWidth = DEFAULT_MENU_WIDTH,
  onReturnFocus,
  onSelect,
  options,
  popoverClassName,
  searchable = false,
  searchPlaceholder = 'Search',
  tabIndex = 0,
  title = ariaLabel,
  triggerClassName,
  value,
}: DaraSelectProps<Value>) {
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState<MenuPosition | null>(null)
  const [query, setQuery] = useState('')
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([])
  const menuId = useId()
  const visibleOptions = filterOptions(options, searchable ? query : '')
  const selectedIndex = Math.max(
    0,
    visibleOptions.findIndex((option) => option.value === value),
  )
  const initialFocusRef = useRef({ searchable, selectedIndex })

  // This effect establishes focus when the menu opens. Filtering must not
  // schedule another frame that can steal focus back from an option.
  useEffect(() => {
    if (!open) {
      return
    }

    const focusFrame = requestAnimationFrame(() => {
      const initialFocus = initialFocusRef.current
      if (initialFocus.searchable) {
        searchRef.current?.focus()
        return
      }
      optionRefs.current[initialFocus.selectedIndex]?.focus()
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
  }, [open])

  const showMenu = () => {
    const trigger = triggerRef.current
    if (!trigger) {
      return
    }
    const bounds = trigger.getBoundingClientRect()
    const fitsBelow =
      bounds.bottom + MENU_GAP + menuHeight <= window.innerHeight
    initialFocusRef.current = { searchable, selectedIndex }
    setQuery('')
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

  const toggleMenu = () => {
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

  const handleSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      closeAndReturn()
      return
    }
    if (event.key === 'Enter') {
      const option = visibleOptions[0]
      if (option) {
        event.preventDefault()
        event.stopPropagation()
        selectValue(option.value)
      }
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      event.stopPropagation()
      optionRefs.current[0]?.focus()
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
      const option = visibleOptions[current]
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
    if (!visibleOptions.length) {
      return
    }
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? visibleOptions.length - 1
          : event.key === 'ArrowDown'
            ? (current + 1 + visibleOptions.length) % visibleOptions.length
            : (current - 1 + visibleOptions.length) % visibleOptions.length
    optionRefs.current[next]?.focus()
  }

  const currentLabel =
    options.find((option) => option.value === value)?.label ?? value
  const triggerClasses = ['dara-select-trigger', triggerClassName]
    .filter(Boolean)
    .join(' ')
  const popoverClasses = [
    'dara-select-popover',
    searchable ? 'dara-select-popover-searchable' : null,
    popoverClassName,
  ]
    .filter(Boolean)
    .join(' ')
  const optionButtons = visibleOptions.map((option, index) => {
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
  })

  return (
    <>
      <DaraButton
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label={`${ariaLabel}: ${currentLabel}`}
        className={triggerClasses}
        disabled={disabled}
        id={id}
        onClick={toggleMenu}
        onKeyDown={handleTriggerKeyDown}
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
              aria-label={searchable ? undefined : ariaLabel}
              className={popoverClasses}
              id={menuId}
              onKeyDown={handleMenuKeyDown}
              ref={menuRef}
              role={searchable ? undefined : 'listbox'}
              style={{ ...position, width: menuWidth, maxHeight: menuHeight }}
            >
              {searchable && (
                <input
                  {...DARA_WRITING_ASSISTANCE_PROPS}
                  aria-label={`Search ${ariaLabel.toLowerCase()}`}
                  className="dara-select-search"
                  onChange={(event) => setQuery(event.target.value)}
                  onKeyDown={handleSearchKeyDown}
                  placeholder={searchPlaceholder}
                  ref={searchRef}
                  type="text"
                  value={query}
                />
              )}
              {searchable && !visibleOptions.length && (
                <p className="dara-select-empty">No matches</p>
              )}
              {searchable ? (
                <div
                  aria-label={ariaLabel}
                  className="dara-select-list"
                  role="listbox"
                >
                  {optionButtons}
                </div>
              ) : (
                optionButtons
              )}
            </div>
          </div>,
          document.body,
        )}
    </>
  )
}

function filterOptions<Value extends string>(
  options: readonly DaraSelectOption<Value>[],
  query: string,
): readonly DaraSelectOption<Value>[] {
  const needle = query.trim().toLowerCase()
  if (!needle) {
    return options
  }
  return options.filter(
    (option) =>
      option.label.toLowerCase().includes(needle) ||
      option.value.toLowerCase().includes(needle) ||
      option.keywords?.some((keyword) =>
        keyword.toLowerCase().includes(needle),
      ),
  )
}
