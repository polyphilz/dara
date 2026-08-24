import { useMemo, type ReactNode } from 'react'
import {
  MarkdownRenderer,
  type MarkdownLinkRenderInput,
} from '../markdown/MarkdownRenderer.tsx'
import {
  ClozeProjection,
  clozeIndexFromVariantKey,
  parseClozeMarkdown,
  projectClozeMarkdown,
} from './cloze.ts'

const CLOZE_PLACEHOLDER_URL =
  'https://dara.invalid/__internal/cloze-placeholder'

interface ClozeMarkdownRendererProps {
  projection: ClozeProjection
  source: string
  variantKey?: string
}

export function ClozeMarkdownRenderer({
  projection,
  source,
  variantKey,
}: ClozeMarkdownRendererProps) {
  const projectedSource = useMemo(() => {
    const document = parseClozeMarkdown(source)
    const selectedIndex = variantKey
      ? clozeIndexFromVariantKey(variantKey)
      : undefined
    return projectClozeMarkdown(
      document,
      projection,
      selectedIndex,
      renderQuestionPlaceholder,
    )
  }, [projection, source, variantKey])

  return (
    <MarkdownRenderer
      renderLink={renderClozePlaceholderLink}
      source={projectedSource}
    />
  )
}

function renderQuestionPlaceholder(placeholderMarkdown: string): string {
  const label = escapeMarkdownLinkLabel(placeholderMarkdown)
  return `[\\[${label}\\]](${CLOZE_PLACEHOLDER_URL})`
}

function escapeMarkdownLinkLabel(value: string): string {
  return value.replace(/\\[\s\S]|]/g, (match) =>
    match === ']' ? '\\]' : match,
  )
}

function renderClozePlaceholderLink({
  children,
  href,
}: MarkdownLinkRenderInput): ReactNode | undefined {
  if (href !== CLOZE_PLACEHOLDER_URL) {
    return undefined
  }
  return (
    <span
      aria-label="Hidden cloze deletion"
      className="dara-cloze-placeholder"
      role="note"
    >
      {children}
    </span>
  )
}
