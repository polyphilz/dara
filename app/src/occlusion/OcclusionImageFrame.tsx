import type { CSSProperties, ReactNode, SVGProps } from 'react'
import { localMediaUrl } from '../media/image-reference.ts'
import type { OcclusionSourceImage } from '../review/contracts.ts'

interface OcclusionImageFrameProps {
  children?: ReactNode
  className?: string
  image: OcclusionSourceImage
  overlayLabel: string
  overlayProps?: SVGProps<SVGSVGElement>
}

export function OcclusionImageFrame({
  children,
  className,
  image,
  overlayLabel,
  overlayProps,
}: OcclusionImageFrameProps) {
  const imageStyle = {
    '--occlusion-image-aspect': image.naturalWidth / image.naturalHeight,
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
