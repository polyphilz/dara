import type { ComponentPropsWithoutRef, ElementType } from 'react'
import {
  DaraTextTone,
  DaraTextVariant,
  type DaraTextTone as DaraTextToneType,
  type DaraTextVariant as DaraTextVariantType,
} from './dara-text-types.ts'
import './dara-text.css'

/**
 * The semantic elements `DaraText` may render. The list is deliberately
 * limited: a call site chooses document semantics here and visual hierarchy
 * through `variant`, so the two decisions never collapse into one.
 */
export type DaraTextElement =
  | 'div'
  | 'span'
  | 'p'
  | 'h1'
  | 'h2'
  | 'h3'
  | 'h4'
  | 'label'
  | 'legend'
  | 'small'
  | 'strong'
  | 'output'

/**
 * `style` is omitted so a call site cannot bypass the typography contract with
 * an inline font declaration. Layout stays with the caller through `className`.
 */
export type DaraTextProps<TElement extends DaraTextElement> = Omit<
  ComponentPropsWithoutRef<TElement>,
  'style' | 'className'
> & {
  as: TElement
  variant: DaraTextVariantType
  tone?: DaraTextToneType
  className?: string
}

const variantClass: Record<DaraTextVariantType, string> = {
  [DaraTextVariant.Display]: 'dara-text-display',
  [DaraTextVariant.Title]: 'dara-text-title',
  [DaraTextVariant.Heading]: 'dara-text-heading',
  [DaraTextVariant.Subheading]: 'dara-text-subheading',
  [DaraTextVariant.Body]: 'dara-text-body',
  [DaraTextVariant.Supporting]: 'dara-text-supporting',
  [DaraTextVariant.Label]: 'dara-text-label',
  [DaraTextVariant.Caption]: 'dara-text-caption',
  [DaraTextVariant.Eyebrow]: 'dara-text-eyebrow',
  [DaraTextVariant.Metric]: 'dara-text-metric',
}

const toneClass: Record<DaraTextToneType, string> = {
  [DaraTextTone.Default]: 'dara-text-tone-default',
  [DaraTextTone.Muted]: 'dara-text-tone-muted',
  [DaraTextTone.Accent]: 'dara-text-tone-accent',
  [DaraTextTone.Success]: 'dara-text-tone-success',
  [DaraTextTone.Warning]: 'dara-text-tone-warning',
  [DaraTextTone.Danger]: 'dara-text-tone-danger',
  [DaraTextTone.Inherit]: 'dara-text-tone-inherit',
}

/** The shared typography primitive for Dara's application chrome. */
export function DaraText<TElement extends DaraTextElement>({
  as,
  className,
  tone = DaraTextTone.Default,
  variant,
  ...props
}: DaraTextProps<TElement>) {
  const Element = as as ElementType

  return (
    <Element
      {...props}
      className={[
        'dara-text',
        variantClass[variant],
        toneClass[tone],
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    />
  )
}
