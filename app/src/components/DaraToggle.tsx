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
    <button
      aria-checked={checked}
      aria-label={label}
      className="dara-toggle"
      disabled={disabled}
      onClick={() => onChange(!checked)}
      role="switch"
      type="button"
    >
      <span aria-hidden="true" />
    </button>
  )
}
