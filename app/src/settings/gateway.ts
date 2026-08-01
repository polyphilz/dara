import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
import type {
  Appearance,
  KeyboardBinding,
  SettingsSnapshot,
} from './types.ts'

export interface SettingsGateway {
  adoptLegacyZoom(expectedRevision: number, zoomPercent: number): Promise<SettingsSnapshot>
  loadSettings(): Promise<SettingsSnapshot>
  setAppearance(expectedRevision: number, appearance: Appearance): Promise<SettingsSnapshot>
  setAutomaticUpdateChecks(
    expectedRevision: number,
    enabled: boolean,
  ): Promise<SettingsSnapshot>
  setKeyboardBindings(
    expectedRevision: number,
    keyboardBindings: KeyboardBinding[],
  ): Promise<SettingsSnapshot>
  setLaunchAtLogin(enabled: boolean): Promise<SettingsSnapshot>
  setZoomPercent(expectedRevision: number, zoomPercent: number): Promise<SettingsSnapshot>
}

export const tauriSettingsGateway: SettingsGateway = {
  adoptLegacyZoom: (expectedRevision, zoomPercent) =>
    invoke<SettingsSnapshot>(DaraIpcCommand.AdoptLegacyZoom, {
      input: { expectedRevision, zoomPercent },
    }),
  loadSettings: () => invoke<SettingsSnapshot>(DaraIpcCommand.LoadSettings),
  setAppearance: (expectedRevision, appearance) =>
    invoke<SettingsSnapshot>(DaraIpcCommand.SetAppearance, {
      input: { appearance, expectedRevision },
    }),
  setAutomaticUpdateChecks: (expectedRevision, enabled) =>
    invoke<SettingsSnapshot>(DaraIpcCommand.SetAutomaticUpdateChecks, {
      input: { enabled, expectedRevision },
    }),
  setKeyboardBindings: (expectedRevision, keyboardBindings) =>
    invoke<SettingsSnapshot>(DaraIpcCommand.SetKeyboardBindings, {
      input: { expectedRevision, keyboardBindings },
    }),
  setLaunchAtLogin: (enabled) =>
    invoke<SettingsSnapshot>(DaraIpcCommand.SetLaunchAtLogin, { input: { enabled } }),
  setZoomPercent: (expectedRevision, zoomPercent) =>
    invoke<SettingsSnapshot>(DaraIpcCommand.SetZoomPercent, {
      input: { expectedRevision, zoomPercent },
    }),
}
