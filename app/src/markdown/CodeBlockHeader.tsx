import { useEffect, useRef, useState } from 'react'
import { DaraButton } from '../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../components/dara-button-types.ts'
import { CodeLanguageMenu } from './CodeLanguageMenu.tsx'

const COPIED_FEEDBACK_MS = 1400

export interface CodeBlockHeaderProps {
  code: () => string
  disabled: boolean
  language: string | null
  onReturnFocus: () => void
  onSelectLanguage: (language: string | null) => void
}

export function CodeBlockHeader({
  code,
  disabled,
  language,
  onReturnFocus,
  onSelectLanguage,
}: CodeBlockHeaderProps) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(
    () => () => {
      if (timer.current) {
        clearTimeout(timer.current)
      }
    },
    [],
  )

  const copy = () => {
    void navigator.clipboard
      .writeText(code())
      .then(() => {
        setCopied(true)
        if (timer.current) {
          clearTimeout(timer.current)
        }
        timer.current = setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS)
      })
      .catch((cause: unknown) => {
        console.error('Could not copy the code block', cause)
      })
  }

  return (
    <div className="dara-code-block-header" contentEditable={false}>
      <CodeLanguageMenu
        disabled={disabled}
        language={language}
        onReturnFocus={onReturnFocus}
        onSelect={onSelectLanguage}
        triggerClassName="dara-code-block-language"
      />
      <DaraButton
        aria-label={copied ? 'Code copied' : 'Copy code'}
        className="dara-code-block-copy"
        onClick={copy}
        size={DaraButtonSize.Custom}
        tabIndex={-1}
        title={copied ? 'Copied' : 'Copy'}
        type="button"
        variant={DaraButtonVariant.Custom}
      >
        {copied ? <CheckIcon /> : <CopyIcon />}
      </DaraButton>
    </div>
  )
}

function CopyIcon() {
  return (
    <svg aria-hidden="true" className="dara-code-block-icon" focusable="false" viewBox="0 0 24 24">
      <rect height="13" rx="2.5" width="13" x="8.5" y="8.5" />
      <path d="M5.5 15.5H5A2.5 2.5 0 0 1 2.5 13V5A2.5 2.5 0 0 1 5 2.5h8A2.5 2.5 0 0 1 15.5 5v.5" />
    </svg>
  )
}

function CheckIcon() {
  return (
    <svg aria-hidden="true" className="dara-code-block-icon" focusable="false" viewBox="0 0 24 24">
      <path d="m4.5 12.5 5 5 10-11" />
    </svg>
  )
}
