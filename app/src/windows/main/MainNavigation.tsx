interface MainNavigationProps {
  disabled?: boolean
  onAdd: () => void
  onBrowse: () => void
  onHome: () => void
  onSettings: () => void
}

export function MainNavigation({
  disabled = false,
  onAdd,
  onBrowse,
  onHome,
  onSettings,
}: MainNavigationProps) {
  return (
    <nav aria-label="Main navigation" className="main-navigation">
      <button disabled={disabled} onClick={onHome} type="button">
        Home
      </button>
      <button disabled={disabled} onClick={onAdd} type="button">
        Add
      </button>
      <button disabled={disabled} onClick={onBrowse} type="button">
        Browse
      </button>
      <button disabled={disabled} onClick={onSettings} type="button">
        Settings
      </button>
    </nav>
  )
}
