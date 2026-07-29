import type { CSSProperties, ReactNode, SVGProps } from 'react'
import { localMediaUrl } from '../media/image-reference.ts'
import type { OcclusionSourceImage } from '../review/contracts.ts'

interface OcclusionImageFrameProps {
  children?: ReactNode
  className?: string
  image: OcclusionSourceImage
  maximumHeight?: number
  overlayLabel: string
  overlayProps?: SVGProps<SVGSVGElement>
}

export function OcclusionImageFrame({
  children,
  className,
  image,
  maximumHeight,
  overlayLabel,
  overlayProps,
}: OcclusionImageFrameProps) {
  const aspectRatio = image.naturalWidth / image.naturalHeight
  const imageStyle = {
    '--occlusion-image-aspect': aspectRatio,
    maxWidth:
      maximumHeight === undefined ? undefined : maximumHeight * aspectRatio,
  } as CSSProperties
  const classes = ['occlusion-image-frame', className]
    .filter(Boolean)
    .join(' ')

  return (
    <div className={classes} style={imageStyle}>
      <div className="occlusion-image-stage">
        <img
          alt="Image occlusion source"
          draggable={false}
          height={image.naturalHeight}
          src={localMediaUrl(image.id)}
          width={image.naturalWidth}
        />
        <svg
          aria-label={overlayLabel}
          preserveAspectRatio="none"
          viewBox={`0 0 ${image.naturalWidth} ${image.naturalHeight}`}
          {...overlayProps}
        >
          {children}
        </svg>
      </div>
    </div>
  )
}
