import { useEffect, useMemo, useState } from 'react'
import {
  OcclusionMaskColor,
  OcclusionMode,
  type OcclusionDefinition,
  type OcclusionMask,
} from '../review/contracts.ts'
import { OcclusionImageFrame } from './OcclusionImageFrame.tsx'

const REVIEW_IMAGE_MAXIMUM_VIEWPORT_RATIO = 0.62

interface OcclusionReviewProps {
  definition: OcclusionDefinition
  revealed: boolean
  targetLayerId: string
}

export function OcclusionReview({
  definition,
  revealed,
  targetLayerId,
}: OcclusionReviewProps) {
  const [peeking, setPeeking] = useState(false)
  const [maximumImageHeight, setMaximumImageHeight] = useState(() =>
    reviewImageMaximumHeight(),
  )
  const target = definition.layers.find((layer) => layer.id === targetLayerId)
  const targetRevealed = revealed || peeking
  const visibleMasks = useMemo(
    () => masksForReview(definition, targetLayerId, targetRevealed),
    [definition, targetRevealed, targetLayerId],
  )
  useEffect(() => {
    setPeeking(false)
  }, [definition.id, revealed, targetLayerId])

  useEffect(() => {
    const fitToViewport = () => {
      setMaximumImageHeight(reviewImageMaximumHeight())
    }
    window.addEventListener('resize', fitToViewport)
    return () => window.removeEventListener('resize', fitToViewport)
  }, [])

  useEffect(() => {
    if (revealed) {
      return
    }
    const toggleFromKeyboard = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.repeat ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.key.toLowerCase() !== 'm'
      ) {
        return
      }
      const active = document.activeElement
      if (
        active instanceof HTMLInputElement ||
        active instanceof HTMLTextAreaElement ||
        active?.getAttribute('contenteditable') === 'true'
      ) {
        return
      }
      event.preventDefault()
      setPeeking((value) => !value)
    }
    window.addEventListener('keydown', toggleFromKeyboard)
    return () => window.removeEventListener('keydown', toggleFromKeyboard)
  }, [revealed])

  if (!target) {
    return <p className="occlusion-render-error">This mask layer is unavailable.</p>
  }

  return (
    <OcclusionImageFrame
      className="occlusion-review-image"
      image={definition.sourceImage}
      maximumHeight={maximumImageHeight}
      overlayLabel={revealed ? 'Image occlusion answer' : 'Image occlusion question'}
      overlayProps={
        revealed
          ? undefined
          : {
              className: 'occlusion-review-overlay',
              onClick: () => setPeeking((value) => !value),
            }
      }
    >
      {visibleMasks.map(({ mask, target: isTarget }) => (
        <rect
          className={[
            'occlusion-review-mask',
            mask.color === OcclusionMaskColor.Black
              ? 'occlusion-mask-black'
              : 'occlusion-mask-white',
            isTarget && definition.mode === OcclusionMode.HideAllGuessOne
              ? 'occlusion-mask-target'
              : '',
          ]
            .filter(Boolean)
            .join(' ')}
          height={mask.height * definition.sourceImage.naturalHeight}
          key={mask.id}
          width={mask.width * definition.sourceImage.naturalWidth}
          x={mask.x * definition.sourceImage.naturalWidth}
          y={mask.y * definition.sourceImage.naturalHeight}
        />
      ))}
    </OcclusionImageFrame>
  )
}

function reviewImageMaximumHeight(): number {
  return Math.max(
    1,
    Math.floor(window.innerHeight * REVIEW_IMAGE_MAXIMUM_VIEWPORT_RATIO),
  )
}

function masksForReview(
  definition: OcclusionDefinition,
  targetLayerId: string,
  revealed: boolean,
): Array<{ mask: OcclusionMask; target: boolean }> {
  if (definition.mode === OcclusionMode.HideOneGuessOne) {
    if (revealed) {
      return []
    }
    return (
      definition.layers
        .find((layer) => layer.id === targetLayerId)
        ?.masks.map((mask) => ({ mask, target: true })) ?? []
    )
  }

  return definition.layers.flatMap((layer) =>
    layer.id === targetLayerId && revealed
      ? []
      : layer.masks.map((mask) => ({
          mask,
          target: layer.id === targetLayerId,
        })),
  )
}
