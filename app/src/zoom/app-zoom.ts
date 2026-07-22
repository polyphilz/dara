import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { tauriSettingsGateway } from '../settings/gateway.ts'
import { DaraEvent } from '../lib/tauri-contracts.ts'
import {
  DEFAULT_ZOOM_PERCENT,
  MAX_ZOOM_PERCENT,
  MIN_ZOOM_PERCENT,
  ZOOM_STEP_PERCENT,
  type SettingsSnapshot,
} from '../settings/types.ts'

export {
  DEFAULT_ZOOM_PERCENT,
  MAX_ZOOM_PERCENT,
  MIN_ZOOM_PERCENT,
  ZOOM_STEP_PERCENT,
} from '../settings/types.ts'

export const AppZoomCommand = {
  ZoomIn: 'ZOOM_IN',
  ZoomOut: 'ZOOM_OUT',
  Reset: 'RESET',
} as const

export type AppZoomCommand =
  (typeof AppZoomCommand)[keyof typeof AppZoomCommand]

const ZOOM_STORAGE_KEY = 'dara.appZoomPercent'
const appZoomCommands = new Set<AppZoomCommand>(Object.values(AppZoomCommand))

let currentZoomPercent = DEFAULT_ZOOM_PERCENT
let currentSettingsRevision = 0

export async function installAppZoom(): Promise<void> {
  const webview = getCurrentWebview()
  const stopCommandListener = await listen<unknown>(
    DaraEvent.ZoomCommand,
    (event) => {
      if (isAppZoomCommand(event.payload)) {
        void executeZoomCommand(event.payload, true)
      }
    },
  )
  const stopChangedListener = await listen<unknown>(
    DaraEvent.SettingsChanged,
    (event) => {
      if (isSettingsSnapshotZoom(event.payload)) {
        currentSettingsRevision = event.payload.revision
        void applyZoomPercent(event.payload.zoomPercent)
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

  let settings = await tauriSettingsGateway.loadSettings()
  if (!settings.legacyZoomMigrated) {
    try {
      settings = await tauriSettingsGateway.adoptLegacyZoom(
        settings.revision,
        readStoredZoomPercent(),
      )
      removeStoredZoomPercent()
    } catch {
      settings = await tauriSettingsGateway.loadSettings()
    }
  }
  currentSettingsRevision = settings.revision
  currentZoomPercent = settings.zoomPercent
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
  _broadcast: boolean,
): Promise<void> {
  const next = zoomPercentForCommand(currentZoomPercent, command)
  try {
    const settings = await tauriSettingsGateway.setZoomPercent(
      currentSettingsRevision,
      next,
    )
    currentSettingsRevision = settings.revision
    await applyZoomPercent(settings.zoomPercent)
  } catch (error) {
    const settings = await tauriSettingsGateway.loadSettings()
    currentSettingsRevision = settings.revision
    await applyZoomPercent(settings.zoomPercent)
    console.error('Could not update app zoom', error)
  }
}

async function applyZoomPercent(percent: number): Promise<void> {
  const next = normalizeZoomPercent(percent)
  if (next !== currentZoomPercent) {
    currentZoomPercent = next
    await getCurrentWebview().setZoom(next / 100)
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

function removeStoredZoomPercent(): void {
  try {
    window.localStorage.removeItem(ZOOM_STORAGE_KEY)
  } catch {
    // The database is authoritative even if legacy cleanup is unavailable.
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

function isSettingsSnapshotZoom(
  value: unknown,
): value is Pick<SettingsSnapshot, 'revision' | 'zoomPercent'> {
  if (!value || typeof value !== 'object') {
    return false
  }
  const candidate = value as { revision?: unknown; zoomPercent?: unknown }
  return (
    typeof candidate.revision === 'number' &&
    Number.isInteger(candidate.revision) &&
    typeof candidate.zoomPercent === 'number' &&
    Number.isFinite(candidate.zoomPercent) &&
    candidate.zoomPercent >= MIN_ZOOM_PERCENT &&
    candidate.zoomPercent <= MAX_ZOOM_PERCENT &&
    candidate.zoomPercent % ZOOM_STEP_PERCENT === 0
  )
}
