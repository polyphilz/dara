import {
  forwardRef,
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from 'react'
import { DaraInput } from '../components/DaraInput.tsx'
import { DaraButton } from '../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../components/dara-button-types.ts'
import { DaraSelect } from '../components/DaraSelect.tsx'
import { createUuidV7 } from '../review/uuid-v7.ts'
import {
  OcclusionMaskColor,
  OcclusionMode,
  type OcclusionDefinition,
  type OcclusionMask,
  type OcclusionMaskLayer,
} from '../review/contracts.ts'
import {
  moveRect,
  normalizedPoint,
  OcclusionResizeHandle,
  rectEquals,
  rectFromPoints,
  resizeRect,
  type NormalizedPoint,
  type NormalizedRect,
  type OcclusionResizeHandle as OcclusionResizeHandleType,
} from './geometry.ts'
import { OcclusionImageFrame } from './OcclusionImageFrame.tsx'

export interface OcclusionEditorHandle {
  focus: () => void
}

interface OcclusionEditorProps {
  definition: OcclusionDefinition
  disabled?: boolean
  onChange: (definition: OcclusionDefinition) => void
  onReplaceImage: () => void
}

const EDITOR_IMAGE_MAXIMUM_HEIGHT = 530

const DrawIntent = {
  NewLayer: 'NEW_LAYER',
  SelectedLayer: 'SELECTED_LAYER',
} as const

type DrawIntent = (typeof DrawIntent)[keyof typeof DrawIntent]

const InteractionKind = {
  Draw: 'DRAW',
  Move: 'MOVE',
  Resize: 'RESIZE',
} as const

type Interaction =
  | {
      kind: typeof InteractionKind.Draw
      base: OcclusionDefinition
      latest: OcclusionDefinition
      intent: DrawIntent
      layerId: string
      maskId: string
      start: NormalizedPoint
    }
  | {
      kind: typeof InteractionKind.Move
      base: OcclusionDefinition
      latest: OcclusionDefinition
      layerId: string
      maskId: string
      original: NormalizedRect
      start: NormalizedPoint
    }
  | {
      kind: typeof InteractionKind.Resize
      base: OcclusionDefinition
      latest: OcclusionDefinition
      handle: OcclusionResizeHandleType
      layerId: string
      maskId: string
      original: NormalizedRect
    }

interface Selection {
  layerId: string
  maskId: string | null
}

const MODE_OPTIONS = [
  { label: 'Hide one, guess one', value: OcclusionMode.HideOneGuessOne },
  { label: 'Hide all, guess one', value: OcclusionMode.HideAllGuessOne },
] as const

const COLOR_OPTIONS = [
  { label: 'White mask', value: OcclusionMaskColor.White },
  { label: 'Black mask', value: OcclusionMaskColor.Black },
] as const

const HISTORY_LIMIT = 100

const LEGEND_ITEMS = [
  { keys: 'Drag', action: 'Draw a mask' },
  { keys: 'Tab / ⇧Tab', action: 'Cycle masks' },
  { keys: 'Enter', action: 'Edit the selected layer' },
  { keys: 'Esc', action: 'Return to mask editing' },
  { keys: 'Arrow keys', action: 'Nudge 1 px' },
  { keys: '⇧ + Arrow', action: 'Nudge 10 px' },
  { keys: 'N', action: 'Draw a new layer' },
  { keys: 'A', action: 'Add a mask to the selected layer' },
  { keys: 'Delete', action: 'Delete the selected mask' },
  { keys: '⌘Z', action: 'Undo' },
] as const

export const OcclusionEditor = forwardRef<
  OcclusionEditorHandle,
  OcclusionEditorProps
>(function OcclusionEditor(
  { definition, disabled = false, onChange, onReplaceImage },
  ref,
) {
  const overlayRef = useRef<SVGSVGElement>(null)
  const editorRef = useRef<HTMLElement>(null)
  const layerLabelRef = useRef<HTMLInputElement>(null)
  const legendTriggerRef = useRef<HTMLButtonElement>(null)
  const legendPopoverRef = useRef<HTMLDivElement>(null)
  const legendReturnFocusRef = useRef<HTMLElement | null>(null)
  const interactionRef = useRef<Interaction | null>(null)
  const legendId = useId()
  const [history, setHistory] = useState<OcclusionDefinition[]>([])
  const [selection, setSelection] = useState<Selection>(() => ({
    layerId: definition.layers[0]?.id ?? '',
    maskId: definition.layers[0]?.masks[0]?.id ?? null,
  }))
  const [drawIntent, setDrawIntent] = useState<DrawIntent>(DrawIntent.NewLayer)
  const [legendOpen, setLegendOpen] = useState(false)

  const focus = useCallback(() => {
    requestAnimationFrame(() =>
      overlayRef.current?.focus({ preventScroll: true }),
    )
  }, [])
  const focusLayerInspector = useCallback(() => {
    requestAnimationFrame(() => layerLabelRef.current?.focus())
  }, [])
  const focusMaskColor = useCallback(() => {
    editorRef.current
      ?.querySelector<HTMLButtonElement>('.occlusion-mask-color-trigger')
      ?.focus()
  }, [])

  const showLegend = useCallback(() => {
    legendReturnFocusRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null
    setLegendOpen(true)
  }, [])

  const hideLegend = useCallback((returnFocus: boolean) => {
    setLegendOpen(false)
    if (returnFocus) {
      requestAnimationFrame(() => {
        const target = legendReturnFocusRef.current
        if (target?.isConnected) {
          target.focus()
        } else {
          legendTriggerRef.current?.focus()
        }
      })
    }
  }, [])
  useImperativeHandle(ref, () => ({ focus }), [focus])

  const selectedLayer =
    definition.layers.find((layer) => layer.id === selection.layerId) ?? null
  const selectedMask =
    selectedLayer?.masks.find((mask) => mask.id === selection.maskId) ?? null

  const pushHistory = useCallback((previous: OcclusionDefinition) => {
    setHistory((current) => [...current.slice(-(HISTORY_LIMIT - 1)), previous])
  }, [])

  const commit = useCallback(
    (next: OcclusionDefinition) => {
      if (next === definition) {
        return
      }
      pushHistory(definition)
      onChange(next)
    },
    [definition, onChange, pushHistory],
  )

  const undo = useCallback(() => {
    const previous = history.at(-1)
    if (!previous || disabled) {
      return
    }
    setHistory((current) => current.slice(0, -1))
    onChange(previous)
    const activeLayer = previous.layers.find(
      (layer) => layer.id === selection.layerId,
    )
    if (!activeLayer) {
      const fallback = previous.layers.at(-1)
      setSelection({
        layerId: fallback?.id ?? '',
        maskId: fallback?.masks[0]?.id ?? null,
      })
    }
  }, [disabled, history, onChange, selection.layerId])

  const beginDraw = (
    event: PointerEvent<SVGSVGElement>,
  ) => {
    if (disabled || event.button !== 0 || event.target !== event.currentTarget) {
      return
    }
    event.currentTarget.focus()
    const point = pointForEvent(event)
    const layerId =
      drawIntent === DrawIntent.SelectedLayer && selectedLayer
        ? selectedLayer.id
        : createUuidV7()
    const maskId = createUuidV7()
    interactionRef.current = {
      kind: InteractionKind.Draw,
      base: definition,
      latest: definition,
      intent:
        drawIntent === DrawIntent.SelectedLayer && selectedLayer
          ? DrawIntent.SelectedLayer
          : DrawIntent.NewLayer,
      layerId,
      maskId,
      start: point,
    }
    event.currentTarget.setPointerCapture(event.pointerId)
    event.preventDefault()
  }

  const beginMove = (
    event: PointerEvent<SVGRectElement>,
    layerId: string,
    mask: OcclusionMask,
  ) => {
    if (disabled || event.button !== 0) {
      return
    }
    const overlay = event.currentTarget.ownerSVGElement
    if (!overlay) {
      return
    }
    overlay.focus()
    setSelection({ layerId, maskId: mask.id })
    interactionRef.current = {
      kind: InteractionKind.Move,
      base: definition,
      latest: definition,
      layerId,
      maskId: mask.id,
      original: mask,
      start: normalizedPoint(
        event.clientX,
        event.clientY,
        overlay.getBoundingClientRect(),
      ),
    }
    overlay.setPointerCapture(event.pointerId)
    event.preventDefault()
    event.stopPropagation()
  }

  const beginResize = (
    event: PointerEvent<SVGCircleElement>,
    handle: OcclusionResizeHandleType,
  ) => {
    if (disabled || event.button !== 0 || !selectedLayer || !selectedMask) {
      return
    }
    const overlay = event.currentTarget.ownerSVGElement
    if (!overlay) {
      return
    }
    overlay.focus()
    interactionRef.current = {
      kind: InteractionKind.Resize,
      base: definition,
      latest: definition,
      handle,
      layerId: selectedLayer.id,
      maskId: selectedMask.id,
      original: selectedMask,
    }
    overlay.setPointerCapture(event.pointerId)
    event.preventDefault()
    event.stopPropagation()
  }

  const updateInteraction = (event: PointerEvent<SVGSVGElement>) => {
    const interaction = interactionRef.current
    if (!interaction) {
      return
    }
    const point = pointForEvent(event)
    if (interaction.kind === InteractionKind.Draw) {
      const rect = rectFromPoints(interaction.start, point)
      if (!rect) {
        interaction.latest = interaction.base
        onChange(interaction.base)
        return
      }
      const mask: OcclusionMask = {
        id: interaction.maskId,
        ...rect,
        color: OcclusionMaskColor.White,
      }
      if (interaction.intent === DrawIntent.SelectedLayer) {
        interaction.latest = updateLayer(
          interaction.base,
          interaction.layerId,
          (layer) => ({
            ...layer,
            masks: [...layer.masks, mask],
          }),
        )
        onChange(interaction.latest)
      } else {
        interaction.latest = {
          ...interaction.base,
          layers: [
            ...interaction.base.layers,
            { id: interaction.layerId, label: null, masks: [mask] },
          ],
        }
        onChange(interaction.latest)
      }
      setSelection({ layerId: interaction.layerId, maskId: interaction.maskId })
      return
    }

    const nextRect =
      interaction.kind === InteractionKind.Move
        ? moveRect(interaction.original, {
            x: point.x - interaction.start.x,
            y: point.y - interaction.start.y,
          })
        : resizeRect(interaction.original, interaction.handle, point)
    interaction.latest = updateMask(
        interaction.base,
        interaction.layerId,
        interaction.maskId,
        (mask) => ({ ...mask, ...nextRect }),
      )
    onChange(interaction.latest)
  }

  const finishInteraction = (event: PointerEvent<SVGSVGElement>) => {
    const interaction = interactionRef.current
    if (!interaction) {
      return
    }
    interactionRef.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    const changed = interactionChanged(interaction, interaction.latest)
    if (changed) {
      pushHistory(interaction.base)
    } else if (interaction.latest !== interaction.base) {
      onChange(interaction.base)
    }
    if (interaction.kind === InteractionKind.Draw) {
      if (!changed) {
        setSelection({ layerId: '', maskId: null })
      }
      setDrawIntent(DrawIntent.NewLayer)
    }
  }

  const removeSelectedMask = useCallback(() => {
    if (!selectedLayer || !selectedMask || disabled) {
      return
    }
    const remainingMasks = selectedLayer.masks.filter(
      (mask) => mask.id !== selectedMask.id,
    )
    if (remainingMasks.length === 0) {
      const remainingLayers = definition.layers.filter(
        (layer) => layer.id !== selectedLayer.id,
      )
      commit({ ...definition, layers: remainingLayers })
      const fallback = remainingLayers.at(-1)
      setSelection({
        layerId: fallback?.id ?? '',
        maskId: fallback?.masks[0]?.id ?? null,
      })
    } else {
      commit(
        updateLayer(definition, selectedLayer.id, (layer) => ({
          ...layer,
          masks: remainingMasks,
        })),
      )
      setSelection({
        layerId: selectedLayer.id,
        maskId: remainingMasks[0]!.id,
      })
    }
  }, [commit, definition, disabled, selectedLayer, selectedMask])

  const removeLayer = (layerId: string) => {
    if (disabled) {
      return
    }
    const layers = definition.layers.filter((layer) => layer.id !== layerId)
    commit({ ...definition, layers })
    const fallback = layers.at(-1)
    setSelection({
      layerId: fallback?.id ?? '',
      maskId: fallback?.masks[0]?.id ?? null,
    })
  }

  const cycleMask = (reverse: boolean) => {
    const masks = definition.layers.flatMap((layer) =>
      layer.masks.map((mask) => ({ layerId: layer.id, maskId: mask.id })),
    )
    if (masks.length === 0) {
      return
    }
    const current = masks.findIndex(
      (mask) =>
        mask.layerId === selection.layerId && mask.maskId === selection.maskId,
    )
    const delta = reverse ? -1 : 1
    const next = masks[(current + delta + masks.length) % masks.length]!
    setSelection(next)
  }

  const handleKeyDown = (event: KeyboardEvent<SVGSVGElement>) => {
    if (event.nativeEvent.isComposing) {
      return
    }
    if (event.metaKey && event.key.toLowerCase() === 'z') {
      event.preventDefault()
      undo()
      return
    }
    if (event.key === 'Tab') {
      event.preventDefault()
      cycleMask(event.shiftKey)
      return
    }
    if (event.key === 'Enter' && selectedLayer && selectedMask) {
      event.preventDefault()
      focusLayerInspector()
      return
    }
    if (event.key === 'Backspace' || event.key === 'Delete') {
      event.preventDefault()
      removeSelectedMask()
      return
    }
    if (event.key.toLowerCase() === 'n') {
      event.preventDefault()
      setDrawIntent(DrawIntent.NewLayer)
      return
    }
    if (event.key.toLowerCase() === 'a' && selectedLayer) {
      event.preventDefault()
      setDrawIntent(DrawIntent.SelectedLayer)
      return
    }
    if (!selectedLayer || !selectedMask || !event.key.startsWith('Arrow')) {
      return
    }
    event.preventDefault()
    const multiplier = event.shiftKey ? 10 : 1
    const delta = {
      x:
        event.key === 'ArrowLeft'
          ? -multiplier / definition.sourceImage.naturalWidth
          : event.key === 'ArrowRight'
            ? multiplier / definition.sourceImage.naturalWidth
            : 0,
      y:
        event.key === 'ArrowUp'
          ? -multiplier / definition.sourceImage.naturalHeight
          : event.key === 'ArrowDown'
            ? multiplier / definition.sourceImage.naturalHeight
            : 0,
    }
    const moved = moveRect(selectedMask, delta)
    if (!rectEquals(selectedMask, moved)) {
      commit(
        updateMask(
          definition,
          selectedLayer.id,
          selectedMask.id,
          (mask) => ({ ...mask, ...moved }),
        ),
      )
    }
  }

  useEffect(() => {
    const selectDrawMode = (event: globalThis.KeyboardEvent) => {
      if (
        disabled ||
        event.defaultPrevented ||
        event.isComposing ||
        event.repeat ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        isEditableTarget(event.target)
      ) {
        return
      }
      const key = event.key.toLowerCase()
      if (key === 'n') {
        event.preventDefault()
        setDrawIntent(DrawIntent.NewLayer)
        focus()
      } else if (key === 'a' && selectedLayer) {
        event.preventDefault()
        setDrawIntent(DrawIntent.SelectedLayer)
        focus()
      }
    }
    window.addEventListener('keydown', selectDrawMode)
    return () => window.removeEventListener('keydown', selectDrawMode)
  }, [disabled, focus, selectedLayer])

  useEffect(() => {
    const toggleLegend = (event: globalThis.KeyboardEvent) => {
      if (
        legendOpen &&
        event.key === 'Escape' &&
        !event.isComposing
      ) {
        event.preventDefault()
        event.stopPropagation()
        hideLegend(true)
        return
      }
      if (
        disabled ||
        event.isComposing ||
        event.repeat ||
        !event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.shiftKey ||
        event.key.toLowerCase() !== 'l'
      ) {
        return
      }
      event.preventDefault()
      if (legendOpen) {
        hideLegend(true)
      } else {
        showLegend()
      }
    }
    window.addEventListener('keydown', toggleLegend, true)
    return () => window.removeEventListener('keydown', toggleLegend, true)
  }, [disabled, hideLegend, legendOpen, showLegend])

  useEffect(() => {
    if (!legendOpen) {
      return
    }
    const closeOnOutsidePointer = (event: globalThis.PointerEvent) => {
      const target = event.target
      if (
        target instanceof Node &&
        !legendTriggerRef.current?.contains(target) &&
        !legendPopoverRef.current?.contains(target)
      ) {
        hideLegend(false)
      }
    }
    document.addEventListener('pointerdown', closeOnOutsidePointer, true)
    return () =>
      document.removeEventListener('pointerdown', closeOnOutsidePointer, true)
  }, [hideLegend, legendOpen])

  const handleRadius = Math.max(
    5,
    Math.min(
      definition.sourceImage.naturalWidth,
      definition.sourceImage.naturalHeight,
    ) * 0.007,
  )
  const maskNumberBadgeRadius = Math.max(
    14,
    Math.min(
      definition.sourceImage.naturalWidth,
      definition.sourceImage.naturalHeight,
    ) * 0.02,
  )

  return (
    <section
      className={`occlusion-editor${
        legendOpen ? ' occlusion-editor-legend-open' : ''
      }`}
      aria-label="Image occlusion editor"
      onKeyDownCapture={(event) => {
        if (
          legendOpen &&
          event.key === 'Escape' &&
          !event.nativeEvent.isComposing
        ) {
          event.preventDefault()
          event.stopPropagation()
          hideLegend(true)
        }
      }}
      ref={editorRef}
    >
      <header className="occlusion-editor-toolbar">
        <div>
          <DaraButton
            className={drawIntent === DrawIntent.NewLayer ? 'active' : undefined}
            disabled={disabled}
            onClick={() => {
              setDrawIntent(DrawIntent.NewLayer)
              focus()
            }}
            size={DaraButtonSize.Compact}
            type="button"
          >
            New layer <kbd>N</kbd>
          </DaraButton>
          <DaraButton
            className={
              drawIntent === DrawIntent.SelectedLayer ? 'active' : undefined
            }
            disabled={disabled || !selectedLayer}
            onClick={() => {
              setDrawIntent(DrawIntent.SelectedLayer)
              focus()
            }}
            size={DaraButtonSize.Compact}
            type="button"
          >
            Add mask <kbd>A</kbd>
          </DaraButton>
          <div className="occlusion-legend">
            <DaraButton
              aria-controls={legendOpen ? legendId : undefined}
              aria-expanded={legendOpen}
              aria-haspopup="dialog"
              aria-keyshortcuts="Meta+L"
              className={legendOpen ? 'active' : undefined}
              disabled={disabled}
              onClick={() => {
                if (legendOpen) {
                  hideLegend(false)
                } else {
                  showLegend()
                }
              }}
              ref={legendTriggerRef}
              size={DaraButtonSize.Compact}
              type="button"
            >
              Legend <kbd>⌘L</kbd>
            </DaraButton>
            {legendOpen && (
              <div
                aria-label="Image occlusion shortcuts"
                className="occlusion-legend-popover"
                id={legendId}
                ref={legendPopoverRef}
                role="dialog"
              >
                <strong>Image editor shortcuts</strong>
                <dl>
                  {LEGEND_ITEMS.map((item) => (
                    <div key={item.keys}>
                      <dt><kbd>{item.keys}</kbd></dt>
                      <dd>{item.action}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            )}
          </div>
        </div>
        <div>
          <DaraSelect
            ariaLabel="Occlusion mode"
            disabled={disabled}
            menuHeight={80}
            menuWidth={190}
            onReturnFocus={focus}
            onSelect={(mode) => commit({ ...definition, mode })}
            options={MODE_OPTIONS}
            value={definition.mode}
          />
          <DaraButton
            disabled={disabled}
            onClick={onReplaceImage}
            size={DaraButtonSize.Compact}
            type="button"
          >
            Replace image
          </DaraButton>
        </div>
      </header>

      <div className="occlusion-editor-workspace">
        <div>
          <OcclusionImageFrame
            className={`occlusion-editor-image occlusion-draw-${drawIntent.toLowerCase()}`}
            image={definition.sourceImage}
            maximumHeight={EDITOR_IMAGE_MAXIMUM_HEIGHT}
            overlayLabel="Editable image masks"
            overlayProps={{
              onKeyDown: handleKeyDown,
              onPointerCancel: finishInteraction,
              onPointerDown: beginDraw,
              onPointerMove: updateInteraction,
              onPointerUp: finishInteraction,
              'aria-keyshortcuts': 'Enter',
              ref: overlayRef,
              role: 'application',
              tabIndex: 0,
            }}
          >
            {definition.layers.flatMap((layer, layerIndex) =>
              layer.masks.map((mask) => {
                const active = mask.id === selectedMask?.id
                const badgeX =
                  (mask.x + mask.width / 2) *
                  definition.sourceImage.naturalWidth
                const badgeY =
                  (mask.y + mask.height / 2) *
                  definition.sourceImage.naturalHeight
                return (
                  <g key={mask.id}>
                    <rect
                      aria-label={`Layer ${layerIndex + 1} mask`}
                      className={[
                        'occlusion-editor-mask',
                        mask.color === OcclusionMaskColor.Black
                          ? 'occlusion-mask-black'
                          : 'occlusion-mask-white',
                        active ? 'selected' : '',
                      ]
                        .filter(Boolean)
                        .join(' ')}
                      height={mask.height * definition.sourceImage.naturalHeight}
                      onPointerDown={(event) => beginMove(event, layer.id, mask)}
                      width={mask.width * definition.sourceImage.naturalWidth}
                      x={mask.x * definition.sourceImage.naturalWidth}
                      y={mask.y * definition.sourceImage.naturalHeight}
                    />
                    <circle
                      aria-hidden="true"
                      className="occlusion-mask-number-badge"
                      cx={badgeX}
                      cy={badgeY}
                      r={maskNumberBadgeRadius}
                    />
                    <text
                      className="occlusion-mask-number"
                      dominantBaseline="central"
                      fontSize={maskNumberBadgeRadius * 1.15}
                      textAnchor="middle"
                      x={badgeX}
                      y={badgeY}
                    >
                      {layerIndex + 1}
                    </text>
                  </g>
                )
              }),
            )}
            {selectedMask &&
              resizeHandlePositions(selectedMask).map(({ handle, x, y }) => (
                <circle
                  className={`occlusion-resize-handle handle-${handle.toLowerCase()}`}
                  cx={x * definition.sourceImage.naturalWidth}
                  cy={y * definition.sourceImage.naturalHeight}
                  key={handle}
                  onPointerDown={(event) => beginResize(event, handle)}
                  r={handleRadius}
                />
              ))}
          </OcclusionImageFrame>
        </div>

        <aside
          className="occlusion-layer-panel"
          aria-label="Mask layers"
          onKeyDown={(event) => {
            if (event.key === 'Escape' && !event.nativeEvent.isComposing) {
              event.preventDefault()
              event.stopPropagation()
              focus()
            }
          }}
        >
          <header>
            <span>Layers</span>
          </header>
          <div className="occlusion-layer-list">
            {definition.layers.map((layer, index) => (
              <DaraButton
                aria-pressed={layer.id === selectedLayer?.id}
                className="occlusion-layer-row"
                key={layer.id}
                onClick={() =>
                  selectMask(layer.id, layer.masks[0]?.id ?? null)
                }
                size={DaraButtonSize.Custom}
                type="button"
              >
                <strong>{index + 1}</strong>
                <span>{layer.label?.trim() || `Layer ${index + 1}`}</span>
                <small>
                  {layer.masks.length}{' '}
                  {layer.masks.length === 1 ? 'mask' : 'masks'}
                </small>
              </DaraButton>
            ))}
          </div>

          {selectedLayer ? (
            <div className="occlusion-layer-controls">
              <label>
                <span>Layer label <small>optional</small></span>
                <DaraInput
                  disabled={disabled}
                  ref={layerLabelRef}
                  onChange={(event) =>
                    commit(
                      updateLayer(definition, selectedLayer.id, (layer) => ({
                        ...layer,
                        label: event.target.value || null,
                      })),
                    )
                  }
                  placeholder={`Layer ${
                    definition.layers.findIndex(
                      (layer) => layer.id === selectedLayer.id,
                    ) + 1
                  }`}
                  type="text"
                  value={selectedLayer.label ?? ''}
                />
              </label>
              <div className="occlusion-mask-tabs" aria-label="Masks in selected layer">
                {selectedLayer.masks.map((mask, index) => (
                  <DaraButton
                    aria-pressed={mask.id === selectedMask?.id}
                    key={mask.id}
                    onClick={() =>
                      selectMask(selectedLayer.id, mask.id)
                    }
                    size={DaraButtonSize.Mini}
                    type="button"
                  >
                    Mask {index + 1}
                  </DaraButton>
                ))}
              </div>
              {selectedMask && (
                <DaraSelect
                  ariaLabel="Mask color"
                  disabled={disabled}
                  menuHeight={80}
                  menuWidth={140}
                  onReturnFocus={focusMaskColor}
                  onSelect={(color) =>
                    commit(
                      updateMask(
                        definition,
                        selectedLayer.id,
                        selectedMask.id,
                        (mask) => ({ ...mask, color }),
                      ),
                    )
                  }
                  options={COLOR_OPTIONS}
                  triggerClassName="occlusion-mask-color-trigger"
                  value={selectedMask.color}
                />
              )}
              <div className="occlusion-delete-actions">
                <DaraButton
                  disabled={disabled || !selectedMask}
                  onClick={removeSelectedMask}
                  size={DaraButtonSize.Compact}
                  type="button"
                  variant={DaraButtonVariant.Danger}
                >
                  Delete mask
                </DaraButton>
                <DaraButton
                  disabled={disabled}
                  onClick={() => removeLayer(selectedLayer.id)}
                  size={DaraButtonSize.Compact}
                  type="button"
                  variant={DaraButtonVariant.Danger}
                >
                  Delete layer
                </DaraButton>
              </div>
            </div>
          ) : (
            <p className="occlusion-layer-empty">Draw a rectangle to create layer 1.</p>
          )}
        </aside>
      </div>
    </section>
  )

  function pointForEvent(event: PointerEvent<SVGSVGElement>): NormalizedPoint {
    return normalizedPoint(
      event.clientX,
      event.clientY,
      event.currentTarget.getBoundingClientRect(),
    )
  }


  function selectMask(layerId: string, maskId: string | null) {
    setSelection({ layerId, maskId })
  }
})

function updateLayer(
  definition: OcclusionDefinition,
  layerId: string,
  update: (layer: OcclusionMaskLayer) => OcclusionMaskLayer,
): OcclusionDefinition {
  return {
    ...definition,
    layers: definition.layers.map((layer) =>
      layer.id === layerId ? update(layer) : layer,
    ),
  }
}

function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  )
}

function updateMask(
  definition: OcclusionDefinition,
  layerId: string,
  maskId: string,
  update: (mask: OcclusionMask) => OcclusionMask,
): OcclusionDefinition {
  return updateLayer(definition, layerId, (layer) => ({
    ...layer,
    masks: layer.masks.map((mask) => (mask.id === maskId ? update(mask) : mask)),
  }))
}

function interactionChanged(
  interaction: Interaction,
  definition: OcclusionDefinition,
): boolean {
  if (interaction.kind === InteractionKind.Draw) {
    return definition.layers.some(
      (layer) =>
        layer.id === interaction.layerId &&
        layer.masks.some((mask) => mask.id === interaction.maskId),
    )
  }
  const mask = definition.layers
    .find((layer) => layer.id === interaction.layerId)
    ?.masks.find((candidate) => candidate.id === interaction.maskId)
  return Boolean(mask && !rectEquals(interaction.original, mask))
}

function resizeHandlePositions(
  mask: OcclusionMask,
): Array<{
  handle: OcclusionResizeHandleType
  x: number
  y: number
}> {
  const left = mask.x
  const centerX = mask.x + mask.width / 2
  const right = mask.x + mask.width
  const top = mask.y
  const centerY = mask.y + mask.height / 2
  const bottom = mask.y + mask.height
  return [
    { handle: OcclusionResizeHandle.NorthWest, x: left, y: top },
    { handle: OcclusionResizeHandle.North, x: centerX, y: top },
    { handle: OcclusionResizeHandle.NorthEast, x: right, y: top },
    { handle: OcclusionResizeHandle.East, x: right, y: centerY },
    { handle: OcclusionResizeHandle.SouthEast, x: right, y: bottom },
    { handle: OcclusionResizeHandle.South, x: centerX, y: bottom },
    { handle: OcclusionResizeHandle.SouthWest, x: left, y: bottom },
    { handle: OcclusionResizeHandle.West, x: left, y: centerY },
  ]
}
