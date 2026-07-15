export const BasicCardFormVariant = {
  Main: 'main',
  Quick: 'quick',
} as const

export type BasicCardFormVariant =
  (typeof BasicCardFormVariant)[keyof typeof BasicCardFormVariant]
