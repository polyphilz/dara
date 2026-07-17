import { invoke } from '@tauri-apps/api/core'
import type {
  Appearance,
  KeyboardBinding,
  SettingsSnapshot,
} from './types.ts'

export interface SettingsGateway {
  adoptLegacyZoom(expectedRevision: number, zoomPercent: number): Promise<SettingsSnapshot>
  loadSettings(): Promise<SettingsSnapshot>
  setAppearance(expectedRevision: number, appearance: Appearance): Promise<SettingsSnapshot>
  setKeyboardBindings(
    expectedRevision: number,
    keyboardBindings: KeyboardBinding[],
  ): Promise<SettingsSnapshot>
  setLaunchAtLogin(enabled: boolean): Promise<SettingsSnapshot>
  setZoomPercent(expectedRevision: number, zoomPercent: number): Promise<SettingsSnapshot>
}

export const tauriSettingsGateway: SettingsGateway = {
  adoptLegacyZoom: (expectedRevision, zoomPercent) =>
    invoke<SettingsSnapshot>('adopt_legacy_zoom', {
      input: { expectedRevision, zoomPercent },
    }),
  loadSettings: () => invoke<SettingsSnapshot>('load_settings'),
  setAppearance: (expectedRevision, appearance) =>
    invoke<SettingsSnapshot>('set_appearance', {
      input: { appearance, expectedRevision },
    }),
  setKeyboardBindings: (expectedRevision, keyboardBindings) =>
    invoke<SettingsSnapshot>('set_keyboard_bindings', {
      input: { expectedRevision, keyboardBindings },
    }),
  setLaunchAtLogin: (enabled) =>
    invoke<SettingsSnapshot>('set_launch_at_login', { input: { enabled } }),
  setZoomPercent: (expectedRevision, zoomPercent) =>
    invoke<SettingsSnapshot>('set_zoom_percent', {
      input: { expectedRevision, zoomPercent },
    }),
}
