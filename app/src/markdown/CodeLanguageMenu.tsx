import {
  DaraSelect,
  type DaraSelectOption,
} from '../components/DaraSelect.tsx'
import {
  codeLanguageAliases,
  codeLanguageDefinitions,
  codeLanguageDisplayName,
} from './languages.ts'

interface CodeLanguageMenuProps {
  disabled: boolean
  language: string | null
  onReturnFocus: () => void
  onSelect: (language: string | null) => void
  tabIndex?: number
  triggerClassName?: string
}

export function CodeLanguageMenu({
  disabled,
  language,
  onReturnFocus,
  onSelect,
  tabIndex = -1,
  triggerClassName = 'toolbar-button code-language-trigger',
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
      searchable
      searchPlaceholder="Search languages"
      tabIndex={tabIndex}
      title="Code language"
      triggerClassName={triggerClassName}
      value={value}
    />
  )
}

function languageOptions(language: string | null): DaraSelectOption<string>[] {
  const known = codeLanguageDefinitions.some(
    (definition) => definition.canonical === language,
  )
  return [
    { label: codeLanguageDisplayName(null), value: '' },
    ...(language && !known
      ? [{ label: codeLanguageDisplayName(language), value: language }]
      : []),
    ...codeLanguageDefinitions.map((definition) => ({
      // Aliases let "ts" find TypeScript and "py" find Python.
      keywords: codeLanguageAliases[definition.canonical] ?? [],
      label: codeLanguageDisplayName(definition.canonical),
      value: definition.canonical,
    })),
  ]
}
