import { DaraButton } from './DaraButton.tsx'
import { DaraButtonSize, DaraButtonVariant } from './dara-button-types.ts'

interface DaraToggleProps {
  checked: boolean
  disabled?: boolean
  label: string
  onChange: (checked: boolean) => void
}

export function DaraToggle({
  checked,
  disabled = false,
  label,
  onChange,
}: DaraToggleProps) {
  return (
    <DaraButton
      aria-checked={checked}
      aria-label={label}
      className="dara-toggle"
      disabled={disabled}
      onClick={() => onChange(!checked)}
      role="switch"
      size={DaraButtonSize.Custom}
      type="button"
      variant={DaraButtonVariant.Custom}
    >
      <span aria-hidden="true" />
    </DaraButton>
  )
}
