export const DaraTextVariant = {
  Display: 'DISPLAY',
  Title: 'TITLE',
  Heading: 'HEADING',
  Subheading: 'SUBHEADING',
  Body: 'BODY',
  Supporting: 'SUPPORTING',
  Label: 'LABEL',
  Caption: 'CAPTION',
  Eyebrow: 'EYEBROW',
  Metric: 'METRIC',
} as const

export type DaraTextVariant =
  (typeof DaraTextVariant)[keyof typeof DaraTextVariant]

export const DaraTextTone = {
  Default: 'DEFAULT',
  Muted: 'MUTED',
  Accent: 'ACCENT',
  Success: 'SUCCESS',
  Warning: 'WARNING',
  Danger: 'DANGER',
  Inherit: 'INHERIT',
} as const

export type DaraTextTone =
  (typeof DaraTextTone)[keyof typeof DaraTextTone]
