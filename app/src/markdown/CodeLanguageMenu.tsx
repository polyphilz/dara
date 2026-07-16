import {
  DaraSelect,
  type DaraSelectOption,
} from '../components/DaraSelect.tsx'
import {
  codeLanguageDefinitions,
  codeLanguageDisplayName,
} from './languages.ts'

interface CodeLanguageMenuProps {
  disabled: boolean
  language: string | null
  onReturnFocus: () => void
  onSelect: (language: string | null) => void
}

export function CodeLanguageMenu({
  disabled,
  language,
  onReturnFocus,
  onSelect,
}: CodeLanguageMenuProps) {
  const value = language ?? ''
  return (
    <DaraSelect
      ariaLabel="Code language"
      disabled={disabled}
      onReturnFocus={onReturnFocus}
      onSelect={(nextLanguage) => onSelect(nextLanguage || null)}
      options={languageOptions(language)}
      popoverClassName="code-language-popover"
      tabIndex={-1}
      title="Code language"
      triggerClassName="toolbar-button code-language-trigger"
      value={value}
    />
  )
}

function languageOptions(language: string | null): DaraSelectOption<string>[] {
  const known = codeLanguageDefinitions.some(
    (definition) => definition.canonical === language,
  )
  return [
    { label: 'Plain code', value: '' },
    ...(language && !known
      ? [{ label: codeLanguageDisplayName(language), value: language }]
      : []),
    ...codeLanguageDefinitions.map((definition) => ({
      label: codeLanguageDisplayName(definition.canonical),
      value: definition.canonical,
    })),
  ]
}
