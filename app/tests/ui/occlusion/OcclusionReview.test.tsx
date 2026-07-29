import { fireEvent, render } from '@testing-library/react'
import { expect, test } from 'vitest'
import { OcclusionReview } from '../../../src/occlusion/OcclusionReview.tsx'
import {
  OcclusionMaskColor,
  OcclusionMode,
  type OcclusionDefinition,
} from '../../../src/review/contracts.ts'
import { ImageOcrStatus } from '../../../src/media/image-reference.ts'

const definition: OcclusionDefinition = {
  id: '01980c8e-6c00-7000-8000-000000000301',
  sourceImage: {
    id: '01980c8e-6c00-7000-8000-000000000302',
    mimeType: 'image/webp',
    naturalWidth: 1000,
    naturalHeight: 500,
    ocrStatus: ImageOcrStatus.Ready,
  },
  mode: OcclusionMode.HideAllGuessOne,
  layers: [
    {
      id: '01980c8e-6c00-7000-8000-000000000303',
      label: 'Target',
      masks: [
        {
          id: '01980c8e-6c00-7000-8000-000000000304',
          x: 0.1,
          y: 0.2,
          width: 0.2,
          height: 0.1,
          color: OcclusionMaskColor.White,
        },
      ],
    },
    {
      id: '01980c8e-6c00-7000-8000-000000000305',
      label: 'Other',
      masks: [
        {
          id: '01980c8e-6c00-7000-8000-000000000306',
          x: 0.55,
          y: 0.4,
          width: 0.15,
          height: 0.12,
          color: OcclusionMaskColor.Black,
        },
      ],
    },
  ],
}

test('hide-all highlights the target on the question and reveals only it on answer', () => {
  const { container, rerender } = render(
    <OcclusionReview
      definition={definition}
      revealed={false}
      targetLayerId={definition.layers[0]!.id}
    />,
  )
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(2)
  expect(container.querySelectorAll('.occlusion-mask-target')).toHaveLength(1)

  rerender(
    <OcclusionReview
      definition={definition}
      revealed
      targetLayerId={definition.layers[0]!.id}
    />,
  )
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(1)
  expect(container.querySelector('.occlusion-mask-black')).toBeTruthy()
})

test('hide-one renders only the target masks until reveal', () => {
  const hideOne = { ...definition, mode: OcclusionMode.HideOneGuessOne }
  const { container, rerender } = render(
    <OcclusionReview
      definition={hideOne}
      revealed={false}
      targetLayerId={hideOne.layers[0]!.id}
    />,
  )
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(1)
  expect(
    container
      .querySelector<HTMLElement>('.occlusion-image-frame')
      ?.style.getPropertyValue('--occlusion-image-aspect'),
  ).toBe('2')
  expect(
    container
      .querySelector<HTMLElement>('.occlusion-image-stage')
      ?.getAttribute('style'),
  ).toBeNull()

  rerender(
    <OcclusionReview
      definition={hideOne}
      revealed
      targetLayerId={hideOne.layers[0]!.id}
    />,
  )
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(0)
})

test('fits the complete review image when the window height changes', () => {
  const originalHeight = window.innerHeight
  const squareDefinition: OcclusionDefinition = {
    ...definition,
    sourceImage: {
      ...definition.sourceImage,
      naturalHeight: 1_000,
    },
  }
  Object.defineProperty(window, 'innerHeight', {
    configurable: true,
    value: 600,
  })
  try {
    const { container } = render(
      <OcclusionReview
        definition={squareDefinition}
        revealed={false}
        targetLayerId={squareDefinition.layers[0]!.id}
      />,
    )
    const frame = container.querySelector<HTMLElement>(
      '.occlusion-review-image',
    )
    expect(frame?.style.maxWidth).toBe('372px')

    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      value: 400,
    })
    fireEvent(window, new Event('resize'))
    expect(frame?.style.maxWidth).toBe('248px')
  } finally {
    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      value: originalHeight,
    })
  }
})

test('click and M toggle a question-side target peek without revealing the answer', () => {
  const { container, getByLabelText } = render(
    <OcclusionReview
      definition={definition}
      revealed={false}
      targetLayerId={definition.layers[0]!.id}
    />,
  )
  const overlay = getByLabelText('Image occlusion question')
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(2)

  fireEvent.click(overlay)
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(1)
  fireEvent.keyDown(window, { key: 'm' })
  expect(container.querySelectorAll('.occlusion-review-mask')).toHaveLength(2)
})
