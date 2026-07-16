import { useMemo } from 'react'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer.tsx'
import {
  ClozeProjection,
  clozeIndexFromVariantKey,
  parseClozeMarkdown,
  projectClozeMarkdown,
} from './cloze.ts'

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
    return projectClozeMarkdown(document, projection, selectedIndex)
  }, [projection, source, variantKey])

  return <MarkdownRenderer source={projectedSource} />
}
