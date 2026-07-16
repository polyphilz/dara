import { useState } from 'react'
import userEvent from '@testing-library/user-event'
import { fireEvent, render } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { OcclusionEditor } from '../../../src/occlusion/OcclusionEditor.tsx'
import {
  OcclusionMaskColor,
  OcclusionMode,
  type OcclusionDefinition,
} from '../../../src/review/contracts.ts'
import { ImageOcrStatus } from '../../../src/media/image-reference.ts'

const initialDefinition: OcclusionDefinition = {
  id: '01980c8e-6c00-7000-8000-000000000301',
  sourceImage: {
    id: '01980c8e-6c00-7000-8000-000000000302',
    mimeType: 'image/webp',
    naturalHeight: 400,
    naturalWidth: 800,
    ocrStatus: ImageOcrStatus.Pending,
  },
  mode: OcclusionMode.HideOneGuessOne,
  layers: [],
}

test('builds multiple layers and multiple masks per layer with local undo', async () => {
  let current = initialDefinition
  function Harness() {
    const [definition, setDefinition] = useState(initialDefinition)
    current = definition
    return (
      <OcclusionEditor
        definition={definition}
        onChange={setDefinition}
        onReplaceImage={vi.fn()}
      />
    )
  }

  const { container, getByRole, getAllByRole, queryByRole } = render(
    <Harness />,
  )
  const legend = getByRole('button', { name: /Legend/ })
  expect(
    queryByRole('dialog', { name: 'Image occlusion shortcuts' }),
  ).toBeNull()
  fireEvent.click(legend)
  expect(
    getByRole('dialog', { name: 'Image occlusion shortcuts' }).textContent,
  ).toContain('Nudge 10 px')
  expect(legend.getAttribute('aria-expanded')).toBe('true')
  fireEvent.pointerDown(document.body)
  expect(
    queryByRole('dialog', { name: 'Image occlusion shortcuts' }),
  ).toBeNull()

  const newLayer = getByRole('button', { name: /New layer/ })
  newLayer.focus()
  fireEvent.keyDown(window, { key: 'l', metaKey: true })
  expect(
    getByRole('dialog', { name: 'Image occlusion shortcuts' }),
  ).toBeTruthy()
  fireEvent.keyDown(window, { key: 'Escape' })
  expect(
    queryByRole('dialog', { name: 'Image occlusion shortcuts' }),
  ).toBeNull()
  await vi.waitFor(() => expect(document.activeElement).toBe(newLayer))
  const overlay = getByRole('application', { name: 'Editable image masks' })
  vi.spyOn(overlay, 'getBoundingClientRect').mockReturnValue({
    bottom: 400,
    height: 400,
    left: 0,
    right: 800,
    top: 0,
    width: 800,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  })

  draw(overlay, 80, 80, 240, 160, 1)
  expect(current.layers).toHaveLength(1)
  expect(current.layers[0]!.masks).toHaveLength(1)
  const maskNumberBadge = container.querySelector<SVGCircleElement>(
    '.occlusion-mask-number-badge',
  )
  expect(maskNumberBadge?.getAttribute('r')).toBe('14')
  const maskNumber = container.querySelector<SVGTextElement>(
    '.occlusion-mask-number',
  )
  expect(Number(maskNumber?.getAttribute('font-size'))).toBeCloseTo(16.1)
  expect(maskNumber?.getAttribute('stroke')).toBeNull()

  fireEvent.keyDown(window, { key: 'a' })
  expect(getByRole('button', { name: /Add mask/ }).classList).toContain('active')
  draw(overlay, 320, 100, 400, 180, 2)
  expect(current.layers).toHaveLength(1)
  expect(current.layers[0]!.masks).toHaveLength(2)

  fireEvent.keyDown(window, { key: 'n' })
  expect(getByRole('button', { name: /New layer/ }).classList).toContain('active')
  draw(overlay, 500, 220, 620, 300, 3)
  expect(current.layers).toHaveLength(2)
  expect(container.querySelectorAll('.occlusion-editor-mask')).toHaveLength(3)
  expect(getAllByRole('button', { name: /Layer [12]/ })).toHaveLength(2)
  expect(getByRole('button', { name: /Layer 1.*2 masks/ })).toBeTruthy()
  expect(getByRole('button', { name: /Layer 2.*1 mask/ })).toBeTruthy()

  fireEvent.mouseDown(getByRole('button', { name: /Occlusion mode/ }))
  fireEvent.mouseDown(getByRole('option', { name: 'Hide all, guess one' }))
  expect(current.mode).toBe(OcclusionMode.HideAllGuessOne)

  expect(getByRole('button', { name: /Legend/ })).toBeTruthy()
  expect(queryByRole('button', { name: /Undo/ })).toBeNull()
  fireEvent.keyDown(overlay, { key: 'z', metaKey: true })
  expect(current.mode).toBe(OcclusionMode.HideOneGuessOne)
  fireEvent.keyDown(overlay, { key: 'z', metaKey: true })
  expect(current.layers).toHaveLength(1)
  expect(container.querySelectorAll('.occlusion-editor-mask')).toHaveLength(2)
})

test('canvas and layer inspector form one complete keyboard loop', async () => {
  const user = userEvent.setup()
  const editableDefinition: OcclusionDefinition = {
    ...initialDefinition,
    layers: [
      {
        id: '01980c8e-6c00-7000-8000-000000000303',
        label: null,
        masks: [
          {
            id: '01980c8e-6c00-7000-8000-000000000304',
            x: 0.1,
            y: 0.2,
            width: 0.2,
            height: 0.2,
            color: OcclusionMaskColor.White,
          },
          {
            id: '01980c8e-6c00-7000-8000-000000000305',
            x: 0.5,
            y: 0.4,
            width: 0.1,
            height: 0.1,
            color: OcclusionMaskColor.Black,
          },
        ],
      },
    ],
  }
  let current = editableDefinition
  function Harness() {
    const [definition, setDefinition] = useState(editableDefinition)
    current = definition
    return (
      <OcclusionEditor
        definition={definition}
        onChange={setDefinition}
        onReplaceImage={vi.fn()}
      />
    )
  }

  const { container, getByRole } = render(<Harness />)
  const overlay = getByRole('application', { name: 'Editable image masks' })
  vi.spyOn(overlay, 'getBoundingClientRect').mockReturnValue({
    bottom: 400,
    height: 400,
    left: 0,
    right: 800,
    top: 0,
    width: 800,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  })

  selectMask(container, overlay, 0, 1)
  expect(document.activeElement).toBe(overlay)

  fireEvent.keyDown(overlay, { key: 'Enter' })
  const label = getByRole('textbox', { name: /Layer label/ })
  await vi.waitFor(() => expect(document.activeElement).toBe(label))
  await user.tab()
  expect(document.activeElement).toBe(
    getByRole('button', { name: 'Mask 1' }),
  )
  await user.tab()
  expect(document.activeElement).toBe(
    getByRole('button', { name: 'Mask 2' }),
  )
  await user.tab()
  const color = getByRole('button', { name: 'Mask color: White mask' })
  expect(document.activeElement).toBe(color)
  await user.keyboard('{Enter}{ArrowDown}{Enter}')
  expect(current.layers[0]!.masks[0]!.color).toBe(OcclusionMaskColor.Black)
  await vi.waitFor(() => expect(document.activeElement).toBe(color))
  await user.tab()
  expect(document.activeElement).toBe(
    getByRole('button', { name: 'Delete mask' }),
  )
  fireEvent.keyDown(getByRole('complementary', { name: 'Mask layers' }), {
    key: 'Escape',
  })
  await vi.waitFor(() => expect(document.activeElement).toBe(overlay))

  fireEvent.keyDown(overlay, { key: 'ArrowRight' })
  expect(current.layers[0]!.masks[0]!.x).toBe(0.1013)
  fireEvent.keyDown(overlay, { key: 'ArrowDown', shiftKey: true })
  expect(current.layers[0]!.masks[0]!.y).toBe(0.225)

  label.focus()
  expect(document.activeElement).toBe(label)
  fireEvent.pointerDown(overlay, {
    button: 0,
    clientX: 740,
    clientY: 360,
    pointerId: 2,
  })
  fireEvent.pointerUp(overlay, {
    clientX: 740,
    clientY: 360,
    pointerId: 2,
  })
  expect(container.querySelector('.occlusion-editor-mask.selected')).toBeNull()
  expect(document.activeElement).toBe(overlay)

  selectMask(container, overlay, 0, 3)
  fireEvent.keyDown(overlay, { key: 'Delete' })
  expect(current.layers).toHaveLength(1)
  expect(current.layers[0]!.masks).toHaveLength(1)

  selectMask(container, overlay, 0, 4)
  fireEvent.keyDown(overlay, { key: 'Delete' })
  expect(current.layers).toHaveLength(0)
})

function draw(
  overlay: HTMLElement,
  startX: number,
  startY: number,
  endX: number,
  endY: number,
  pointerId: number,
) {
  fireEvent.pointerDown(overlay, {
    button: 0,
    clientX: startX,
    clientY: startY,
    pointerId,
  })
  fireEvent.pointerMove(overlay, {
    clientX: endX,
    clientY: endY,
    pointerId,
  })
  fireEvent.pointerUp(overlay, {
    clientX: endX,
    clientY: endY,
    pointerId,
  })
}

function selectMask(
  container: HTMLElement,
  overlay: HTMLElement,
  index: number,
  pointerId: number,
) {
  const mask = container.querySelectorAll<SVGRectElement>(
    '.occlusion-editor-mask',
  )[index]
  if (!mask) {
    throw new Error(`Mask ${index} not found`)
  }
  fireEvent.pointerDown(mask, {
    button: 0,
    clientX: 100,
    clientY: 100,
    pointerId,
  })
  fireEvent.pointerUp(overlay, {
    clientX: 100,
    clientY: 100,
    pointerId,
  })
}
