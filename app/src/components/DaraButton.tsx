import {
  forwardRef,
  type ComponentPropsWithoutRef,
} from 'react'
import {
  DaraButtonSize,
  DaraButtonVariant,
  type DaraButtonSize as DaraButtonSizeType,
  type DaraButtonVariant as DaraButtonVariantType,
} from './dara-button-types.ts'
import './dara-button.css'

export interface DaraButtonProps extends ComponentPropsWithoutRef<'button'> {
  size?: DaraButtonSizeType
  variant?: DaraButtonVariantType
}

const sizeClass: Record<DaraButtonSizeType, string> = {
  [DaraButtonSize.Standard]: 'dara-button-standard',
  [DaraButtonSize.Compact]: 'dara-button-compact',
  [DaraButtonSize.Mini]: 'dara-button-mini',
  [DaraButtonSize.Icon]: 'dara-button-icon',
  [DaraButtonSize.Custom]: 'dara-button-custom-size',
}

const variantClass: Record<DaraButtonVariantType, string> = {
  [DaraButtonVariant.Surface]: 'dara-button-surface',
  [DaraButtonVariant.Ghost]: 'dara-button-ghost',
  [DaraButtonVariant.Primary]: 'dara-button-primary',
  [DaraButtonVariant.Accent]: 'dara-button-accent',
  [DaraButtonVariant.Danger]: 'dara-button-danger',
  [DaraButtonVariant.Custom]: 'dara-button-custom-variant',
}

/** The shared native button primitive for Dara controls and actions. */
export const DaraButton = forwardRef<HTMLButtonElement, DaraButtonProps>(
  function DaraButton(
    {
      className,
      size = DaraButtonSize.Standard,
      type = 'button',
      variant = DaraButtonVariant.Surface,
      ...props
    },
    ref,
  ) {
    return (
      <button
        {...props}
        className={[
          'dara-button',
          sizeClass[size],
          variantClass[variant],
          className,
        ]
          .filter(Boolean)
          .join(' ')}
        ref={ref}
        type={type}
      />
    )
  },
)
