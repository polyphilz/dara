import { OcclusionLayerVariantPrefix } from '../review/contracts.ts'

export function occlusionLayerId(variantKey: string): string | null {
  return variantKey.startsWith(OcclusionLayerVariantPrefix)
    ? variantKey.slice(OcclusionLayerVariantPrefix.length)
    : null
}
