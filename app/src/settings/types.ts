export const Appearance = {
  System: 'SYSTEM',
  Light: 'LIGHT',
  Dark: 'DARK',
} as const

export type Appearance = (typeof Appearance)[keyof typeof Appearance]

export const DaraCommand = {
  QuickAdd: 'QUICK_ADD',
  Home: 'HOME',
} as const

export type DaraCommand = (typeof DaraCommand)[keyof typeof DaraCommand]

export interface KeyboardBinding {
  accelerator: string
  command: DaraCommand
}

export interface SettingsSnapshot {
  appearance: Appearance
  desiredRetention: number
  keyboardBindings: KeyboardBinding[]
  launchAtLogin: boolean
  launchAtLoginError: string | null
  legacyZoomMigrated: boolean
  revision: number
  shortcutErrors: string[]
  zoomPercent: number
}

export const DEFAULT_QUICK_ADD_ACCELERATOR = 'control+alt+super+KeyD'
export const DEFAULT_HOME_ACCELERATOR = 'control+alt+super+KeyH'
export const DEFAULT_ZOOM_PERCENT = 100
export const MIN_ZOOM_PERCENT = 50
export const MAX_ZOOM_PERCENT = 200
export const ZOOM_STEP_PERCENT = 10
export const SETTINGS_CHANGED_EVENT = 'settings-changed'

export const DEFAULT_KEYBOARD_BINDINGS: readonly KeyboardBinding[] = [
  {
    accelerator: DEFAULT_QUICK_ADD_ACCELERATOR,
    command: DaraCommand.QuickAdd,
  },
  {
    accelerator: DEFAULT_HOME_ACCELERATOR,
    command: DaraCommand.Home,
  },
]
