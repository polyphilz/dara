import { DaraButton } from '../../components/DaraButton.tsx'
import { DaraButtonVariant } from '../../components/dara-button-types.ts'

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
      <DaraButton
        disabled={disabled}
        onClick={onHome}
        type="button"
        variant={DaraButtonVariant.Ghost}
      >
        Home
      </DaraButton>
      <DaraButton
        disabled={disabled}
        onClick={onAdd}
        type="button"
        variant={DaraButtonVariant.Ghost}
      >
        Add
      </DaraButton>
      <DaraButton
        disabled={disabled}
        onClick={onBrowse}
        type="button"
        variant={DaraButtonVariant.Ghost}
      >
        Browse
      </DaraButton>
      <DaraButton
        disabled={disabled}
        onClick={onSettings}
        type="button"
        variant={DaraButtonVariant.Ghost}
      >
        Settings
      </DaraButton>
    </nav>
  )
}
