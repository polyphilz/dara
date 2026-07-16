import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'

export const AppZoomCommand = {
  ZoomIn: 'ZOOM_IN',
  ZoomOut: 'ZOOM_OUT',
  Reset: 'RESET',
} as const

export type AppZoomCommand =
  (typeof AppZoomCommand)[keyof typeof AppZoomCommand]

export const DEFAULT_ZOOM_PERCENT = 100
export const MIN_ZOOM_PERCENT = 50
export const MAX_ZOOM_PERCENT = 200
export const ZOOM_STEP_PERCENT = 10

const ZOOM_STORAGE_KEY = 'dara.appZoomPercent'
const ZOOM_COMMAND_EVENT = 'app-zoom-command'
const ZOOM_CHANGED_EVENT = 'app-zoom-changed'
const appZoomCommands = new Set<AppZoomCommand>(Object.values(AppZoomCommand))

let currentZoomPercent = DEFAULT_ZOOM_PERCENT

export async function installAppZoom(): Promise<void> {
  const webview = getCurrentWebview()
  const stopCommandListener = await listen<unknown>(
    ZOOM_COMMAND_EVENT,
    (event) => {
      if (isAppZoomCommand(event.payload)) {
        void executeZoomCommand(event.payload, true)
      }
    },
  )
  const stopChangedListener = await listen<unknown>(
    ZOOM_CHANGED_EVENT,
    (event) => {
      if (isZoomPercent(event.payload)) {
        void applyZoomPercent(event.payload, true)
      }
    },
  )

  const handleKeyDown = (event: KeyboardEvent) => {
    const command = zoomCommandForKeyboardEvent(event)
    if (command === null) {
      return
    }
    event.preventDefault()
    void executeZoomCommand(command, true)
  }
  window.addEventListener('keydown', handleKeyDown, { capture: true })

  currentZoomPercent = readStoredZoomPercent()
  await webview.setZoom(currentZoomPercent / 100)

  window.addEventListener(
    'pagehide',
    () => {
      stopCommandListener()
      stopChangedListener()
      window.removeEventListener('keydown', handleKeyDown, { capture: true })
    },
    { once: true },
  )
}

export function zoomPercentForCommand(
  current: number,
  command: AppZoomCommand,
): number {
  switch (command) {
    case AppZoomCommand.ZoomIn:
      return normalizeZoomPercent(current + ZOOM_STEP_PERCENT)
    case AppZoomCommand.ZoomOut:
      return normalizeZoomPercent(current - ZOOM_STEP_PERCENT)
    case AppZoomCommand.Reset:
      return DEFAULT_ZOOM_PERCENT
  }
}

export function zoomCommandForKeyboardEvent(
  event: Pick<
    KeyboardEvent,
    'altKey' | 'code' | 'ctrlKey' | 'metaKey'
  >,
): AppZoomCommand | null {
  if (!event.metaKey || event.altKey || event.ctrlKey) {
    return null
  }
  switch (event.code) {
    case 'Equal':
    case 'NumpadAdd':
      return AppZoomCommand.ZoomIn
    case 'Minus':
    case 'NumpadSubtract':
      return AppZoomCommand.ZoomOut
    case 'Digit0':
    case 'Numpad0':
      return AppZoomCommand.Reset
    default:
      return null
  }
}

async function executeZoomCommand(
  command: AppZoomCommand,
  broadcast: boolean,
): Promise<void> {
  const next = zoomPercentForCommand(currentZoomPercent, command)
  await applyZoomPercent(next, true)
  if (broadcast) {
    await emit(ZOOM_CHANGED_EVENT, next)
  }
}

async function applyZoomPercent(
  percent: number,
  persist: boolean,
): Promise<void> {
  const next = normalizeZoomPercent(percent)
  if (next !== currentZoomPercent) {
    currentZoomPercent = next
    await getCurrentWebview().setZoom(next / 100)
  }
  if (persist) {
    storeZoomPercent(next)
  }
}

function readStoredZoomPercent(): number {
  try {
    const stored = window.localStorage.getItem(ZOOM_STORAGE_KEY)
    return stored === null
      ? DEFAULT_ZOOM_PERCENT
      : normalizeZoomPercent(Number(stored))
  } catch {
    return DEFAULT_ZOOM_PERCENT
  }
}

function storeZoomPercent(percent: number): void {
  try {
    window.localStorage.setItem(ZOOM_STORAGE_KEY, String(percent))
  } catch {
    // Zoom still works for this session if persistence is unavailable.
  }
}

function normalizeZoomPercent(percent: number): number {
  if (!Number.isFinite(percent)) {
    return DEFAULT_ZOOM_PERCENT
  }
  const rounded = Math.round(percent / ZOOM_STEP_PERCENT) * ZOOM_STEP_PERCENT
  return Math.min(MAX_ZOOM_PERCENT, Math.max(MIN_ZOOM_PERCENT, rounded))
}

function isAppZoomCommand(value: unknown): value is AppZoomCommand {
  return typeof value === 'string' && appZoomCommands.has(value as AppZoomCommand)
}

function isZoomPercent(value: unknown): value is number {
  return (
    typeof value === 'number' &&
    Number.isFinite(value) &&
    value >= MIN_ZOOM_PERCENT &&
    value <= MAX_ZOOM_PERCENT &&
    value % ZOOM_STEP_PERCENT === 0
  )
}
