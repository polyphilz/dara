interface DaraImageIconProps {
  className?: string
}

/** Dara's framed-landscape symbol for image insertion controls. */
export function DaraImageIcon({ className }: DaraImageIconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      focusable="false"
      viewBox="0 0 24 24"
    >
      <rect height="16" rx="2" width="18" x="3" y="4" />
      <circle cx="8.5" cy="9" r="1.5" />
      <path d="m4 18 4.5-4.5 3.5 3.5 2.5-2.5 5.5 5.5" />
    </svg>
  )
}
