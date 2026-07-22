import { listen } from '@tauri-apps/api/event'
import { tauriSettingsGateway } from './gateway.ts'
import { DaraEvent } from '../lib/tauri-contracts.ts'
import {
  Appearance,
  type SettingsSnapshot,
} from './types.ts'

const appearances = new Set<Appearance>(Object.values(Appearance))

export async function installAppAppearance(): Promise<void> {
  applyAppearance((await tauriSettingsGateway.loadSettings()).appearance)
  const stopListening = await listen<unknown>(
    DaraEvent.SettingsChanged,
    (event) => {
      if (isSettingsSnapshotAppearance(event.payload)) {
        applyAppearance(event.payload.appearance)
      }
    },
  )
  window.addEventListener('pagehide', stopListening, { once: true })
}

export function applyAppearance(appearance: Appearance): void {
  document.documentElement.dataset.appearance = appearance
  document.documentElement.style.colorScheme =
    appearance === Appearance.System
      ? 'light dark'
      : appearance === Appearance.Dark
        ? 'dark'
        : 'light'
}

function isSettingsSnapshotAppearance(
  value: unknown,
): value is Pick<SettingsSnapshot, 'appearance'> {
  if (!value || typeof value !== 'object') {
    return false
  }
  const appearance = (value as { appearance?: unknown }).appearance
  return (
    typeof appearance === 'string' &&
    appearances.has(appearance as Appearance)
  )
}
