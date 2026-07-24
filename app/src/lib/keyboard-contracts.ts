export const DaraSurface = {
  Browse: 'BROWSE',
  Main: 'MAIN',
  QuickAdd: 'QUICK_ADD',
  Review: 'REVIEW',
  Settings: 'SETTINGS',
} as const

export type DaraSurface =
  (typeof DaraSurface)[keyof typeof DaraSurface]

export const KeyboardRoute = {
  Dom: 'DOM',
  GlobalShortcut: 'GLOBAL_SHORTCUT',
  NativeMenu: 'NATIVE_MENU',
  TauriEvent: 'TAURI_EVENT',
} as const

export type KeyboardRoute =
  (typeof KeyboardRoute)[keyof typeof KeyboardRoute]

export const KeyboardAction = {
  BrowseFocusSearch: 'BROWSE_FOCUS_SEARCH',
  BrowseToggleSuspension: 'BROWSE_TOGGLE_SUSPENSION',
  OpenHome: 'OPEN_HOME',
  OpenQuickAdd: 'OPEN_QUICK_ADD',
  OpenSettings: 'OPEN_SETTINGS',
  ReviewDirectGrade: 'REVIEW_DIRECT_GRADE',
  ReviewMoveGradeFocus: 'REVIEW_MOVE_GRADE_FOCUS',
  ReviewReveal: 'REVIEW_REVEAL',
  ReviewSubmitFocusedGrade: 'REVIEW_SUBMIT_FOCUSED_GRADE',
  ReviewUndo: 'REVIEW_UNDO',
  ZoomIn: 'ZOOM_IN',
  ZoomOut: 'ZOOM_OUT',
  ZoomReset: 'ZOOM_RESET',
} as const

export type KeyboardAction =
  (typeof KeyboardAction)[keyof typeof KeyboardAction]

export interface KeyboardContract {
  action: KeyboardAction
  contract: string
  route: KeyboardRoute
  surface: DaraSurface
}

export const KEYBOARD_CONTRACTS = [
  { action: KeyboardAction.OpenQuickAdd, contract: 'Control+Alt+Meta+KeyD', route: KeyboardRoute.GlobalShortcut, surface: DaraSurface.Main },
  { action: KeyboardAction.OpenHome, contract: 'Control+Alt+Meta+KeyH', route: KeyboardRoute.GlobalShortcut, surface: DaraSurface.Main },
  { action: KeyboardAction.OpenSettings, contract: 'Meta+Comma', route: KeyboardRoute.Dom, surface: DaraSurface.Main },
  { action: KeyboardAction.ReviewReveal, contract: 'Space keydown; not repeat or composing', route: KeyboardRoute.Dom, surface: DaraSurface.Review },
  { action: KeyboardAction.ReviewMoveGradeFocus, contract: 'Tab or Shift+Tab after reveal; repeat allowed; clamp', route: KeyboardRoute.Dom, surface: DaraSurface.Review },
  { action: KeyboardAction.ReviewSubmitFocusedGrade, contract: 'Enter, or Space after reveal keyup; not repeat or composing', route: KeyboardRoute.Dom, surface: DaraSurface.Review },
  { action: KeyboardAction.ReviewDirectGrade, contract: 'Digit1 through Digit4 after reveal; not repeat or composing', route: KeyboardRoute.Dom, surface: DaraSurface.Review },
  { action: KeyboardAction.ReviewUndo, contract: 'Meta+KeyZ when undo is available', route: KeyboardRoute.Dom, surface: DaraSurface.Review },
  { action: KeyboardAction.BrowseFocusSearch, contract: 'Meta+KeyF', route: KeyboardRoute.Dom, surface: DaraSurface.Browse },
  { action: KeyboardAction.BrowseToggleSuspension, contract: 'Meta+KeyJ', route: KeyboardRoute.Dom, surface: DaraSurface.Browse },
  { action: KeyboardAction.ZoomIn, contract: 'Meta+Equal or Meta+NumpadAdd', route: KeyboardRoute.Dom, surface: DaraSurface.Main },
  { action: KeyboardAction.ZoomOut, contract: 'Meta+Minus or Meta+NumpadSubtract', route: KeyboardRoute.Dom, surface: DaraSurface.Main },
  { action: KeyboardAction.ZoomReset, contract: 'Meta+Digit0 or Meta+Numpad0', route: KeyboardRoute.Dom, surface: DaraSurface.Main },
] as const satisfies readonly KeyboardContract[]
