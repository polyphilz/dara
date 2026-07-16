export const CardFormVariant = {
  Main: 'main',
  Quick: 'quick',
} as const

export type CardFormVariant =
  (typeof CardFormVariant)[keyof typeof CardFormVariant]
