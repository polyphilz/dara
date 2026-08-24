export const BrowserScenarioId = {
  MainBrowseBasic: 'MAIN_BROWSE_BASIC',
  MainBrowseDeepRoute: 'MAIN_BROWSE_DEEP_ROUTE',
  MainBrowseHistory: 'MAIN_BROWSE_HISTORY',
  MainReviewBasic: 'MAIN_REVIEW_BASIC',
  MainReviewCloze: 'MAIN_REVIEW_CLOZE',
  QuickAddCreateFailsOnce: 'QUICK_ADD_CREATE_FAILS_ONCE',
  QuickAddEmpty: 'QUICK_ADD_EMPTY',
} as const

export type BrowserScenarioId =
  (typeof BrowserScenarioId)[keyof typeof BrowserScenarioId]

export interface BrowserScenario {
  id: BrowserScenarioId
}

export const BrowserHarnessSurface = {
  Main: 'MAIN',
  QuickAdd: 'QUICK_ADD',
  Recovery: 'RECOVERY',
  VisualCatalog: 'VISUAL_CATALOG',
} as const

export type BrowserHarnessSurface =
  (typeof BrowserHarnessSurface)[keyof typeof BrowserHarnessSurface]

export const browserScenarios: Record<BrowserScenarioId, BrowserScenario> = {
  [BrowserScenarioId.MainBrowseBasic]: {
    id: BrowserScenarioId.MainBrowseBasic,
  },
  [BrowserScenarioId.MainBrowseDeepRoute]: {
    id: BrowserScenarioId.MainBrowseDeepRoute,
  },
  [BrowserScenarioId.MainBrowseHistory]: {
    id: BrowserScenarioId.MainBrowseHistory,
  },
  [BrowserScenarioId.MainReviewBasic]: {
    id: BrowserScenarioId.MainReviewBasic,
  },
  [BrowserScenarioId.MainReviewCloze]: {
    id: BrowserScenarioId.MainReviewCloze,
  },
  [BrowserScenarioId.QuickAddCreateFailsOnce]: {
    id: BrowserScenarioId.QuickAddCreateFailsOnce,
  },
  [BrowserScenarioId.QuickAddEmpty]: {
    id: BrowserScenarioId.QuickAddEmpty,
  },
}

export function parseBrowserScenario(value: string | null): BrowserScenario {
  if (
    value === BrowserScenarioId.QuickAddEmpty ||
    value === null
  ) {
    return browserScenarios[BrowserScenarioId.QuickAddEmpty]
  }
  if (value === BrowserScenarioId.MainReviewBasic) {
    return browserScenarios[BrowserScenarioId.MainReviewBasic]
  }
  if (value === BrowserScenarioId.MainReviewCloze) {
    return browserScenarios[BrowserScenarioId.MainReviewCloze]
  }
  if (value === BrowserScenarioId.MainBrowseBasic) {
    return browserScenarios[BrowserScenarioId.MainBrowseBasic]
  }
  if (value === BrowserScenarioId.MainBrowseDeepRoute) {
    return browserScenarios[BrowserScenarioId.MainBrowseDeepRoute]
  }
  if (value === BrowserScenarioId.MainBrowseHistory) {
    return browserScenarios[BrowserScenarioId.MainBrowseHistory]
  }
  if (value === BrowserScenarioId.QuickAddCreateFailsOnce) {
    return browserScenarios[BrowserScenarioId.QuickAddCreateFailsOnce]
  }
  throw new Error(`Unknown browser scenario: ${value}`)
}

export function parseBrowserHarnessSurface(
  value: string | null,
): BrowserHarnessSurface {
  if (value === BrowserHarnessSurface.VisualCatalog) {
    return BrowserHarnessSurface.VisualCatalog
  }
  if (value === BrowserHarnessSurface.Main) {
    return BrowserHarnessSurface.Main
  }
  if (value === BrowserHarnessSurface.Recovery) {
    return BrowserHarnessSurface.Recovery
  }
  if (value === BrowserHarnessSurface.QuickAdd || value === null) {
    return BrowserHarnessSurface.QuickAdd
  }
  throw new Error(`Unknown browser harness surface: ${value}`)
}
