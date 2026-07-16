interface MainNavigationProps {
  onAdd: () => void
  onBrowse: () => void
  onHome: () => void
}

export function MainNavigation({
  onAdd,
  onBrowse,
  onHome,
}: MainNavigationProps) {
  return (
    <nav aria-label="Main navigation" className="main-navigation">
      <button onClick={onHome} type="button">
        Home
      </button>
      <button onClick={onAdd} type="button">
        Add
      </button>
      <button onClick={onBrowse} type="button">
        Browse
      </button>
    </nav>
  )
}
