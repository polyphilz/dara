import { DaraButton } from './DaraButton.tsx'
import { DaraButtonSize } from './dara-button-types.ts'

interface DaraPercentageControlProps {
  describedBy?: string
  disabled?: boolean
  label: string
  max: number
  min: number
  onChange: (value: number) => void
  value: number
}

export function DaraPercentageControl({
  describedBy,
  disabled = false,
  label,
  max,
  min,
  onChange,
  value,
}: DaraPercentageControlProps) {
  const commit = (next: number) => onChange(Math.min(max, Math.max(min, next)))
  return (
    <div className="percentage-control">
      <DaraButton
        aria-label={`Decrease ${label}`}
        disabled={disabled || value <= min}
        onClick={() => commit(value - 1)}
        size={DaraButtonSize.Icon}
        type="button"
      >
        −
      </DaraButton>
      <input
        aria-describedby={describedBy}
        aria-label={label}
        disabled={disabled}
        max={max}
        min={min}
        onChange={(event) => commit(Number(event.target.value))}
        step={1}
        type="range"
        value={value}
      />
      <output aria-live="polite" htmlFor="desired-retention">
        {value}%
      </output>
      <DaraButton
        aria-label={`Increase ${label}`}
        disabled={disabled || value >= max}
        onClick={() => commit(value + 1)}
        size={DaraButtonSize.Icon}
        type="button"
      >
        +
      </DaraButton>
    </div>
  )
}
