export const DaraButtonSize = {
  Standard: 'STANDARD',
  Compact: 'COMPACT',
  Mini: 'MINI',
  Icon: 'ICON',
  Custom: 'CUSTOM',
} as const

export type DaraButtonSize =
  (typeof DaraButtonSize)[keyof typeof DaraButtonSize]

export const DaraButtonVariant = {
  Surface: 'SURFACE',
  Ghost: 'GHOST',
  Primary: 'PRIMARY',
  Accent: 'ACCENT',
  Danger: 'DANGER',
  Custom: 'CUSTOM',
} as const

export type DaraButtonVariant =
  (typeof DaraButtonVariant)[keyof typeof DaraButtonVariant]
