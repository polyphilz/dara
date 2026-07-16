import type { Node as ProseMirrorNode } from 'prosemirror-model'
import { NodeSelection } from 'prosemirror-state'
import type { EditorView, NodeView } from 'prosemirror-view'
import {
  clampImageDisplayWidth,
  localMediaUrl,
} from '../media/image-reference.ts'

export function imageNodeView(
  initialNode: ProseMirrorNode,
  view: EditorView,
  getPos: () => number | undefined,
): NodeView {
  let node = initialNode
  const dom = document.createElement('figure')
  dom.className = 'dara-editor-image'
  dom.contentEditable = 'false'
  dom.setAttribute('aria-label', 'Pasted card image')

  const image = document.createElement('img')
  image.alt = ''
  image.draggable = false
  dom.append(image)

  const handle = document.createElement('span')
  handle.className = 'dara-editor-image-resize-handle'
  handle.setAttribute('aria-hidden', 'true')
  dom.append(handle)

  const applyNode = (nextNode: ProseMirrorNode) => {
    node = nextNode
    dom.style.width = `${node.attrs.displayWidthPercent}%`
    image.src = localMediaUrl(node.attrs.imageId)
  }
  applyNode(node)

  const beginResize = (event: PointerEvent) => {
    if (!view.editable || event.button !== 0) {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    const parentWidth = dom.parentElement?.getBoundingClientRect().width ?? 0
    if (parentWidth <= 0) {
      return
    }
    const startX = event.clientX
    const startWidth = Number(node.attrs.displayWidthPercent)
    let nextWidth = startWidth
    dom.classList.add('dara-editor-image-resizing')
    handle.setPointerCapture?.(event.pointerId)

    const move = (moveEvent: PointerEvent) => {
      nextWidth = clampImageDisplayWidth(
        startWidth + ((moveEvent.clientX - startX) / parentWidth) * 100,
      )
      dom.style.width = `${nextWidth}%`
    }
    const finish = (finishEvent: PointerEvent) => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', finish)
      handle.removeEventListener('pointercancel', cancel)
      handle.releasePointerCapture?.(finishEvent.pointerId)
      dom.classList.remove('dara-editor-image-resizing')
      const position = getPos()
      if (position === undefined || nextWidth === startWidth) {
        dom.style.width = `${startWidth}%`
        return
      }
      view.dispatch(
        view.state.tr.setNodeMarkup(position, undefined, {
          ...node.attrs,
          displayWidthPercent: nextWidth,
        }),
      )
    }
    const cancel = (cancelEvent: PointerEvent) => {
      nextWidth = startWidth
      finish(cancelEvent)
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', finish)
    handle.addEventListener('pointercancel', cancel)
  }
  handle.addEventListener('pointerdown', beginResize)

  return {
    dom,
    update(nextNode) {
      if (nextNode.type !== node.type) {
        return false
      }
      applyNode(nextNode)
      return true
    },
    selectNode() {
      dom.classList.add('dara-editor-image-selected')
    },
    deselectNode() {
      dom.classList.remove('dara-editor-image-selected')
    },
    stopEvent(event) {
      return event.target === handle || handle.contains(event.target as Node)
    },
    destroy() {
      handle.removeEventListener('pointerdown', beginResize)
    },
  }
}

export function pendingImageNodeView(): NodeView {
  const dom = document.createElement('div')
  dom.className = 'dara-editor-image-pending'
  dom.contentEditable = 'false'
  dom.setAttribute('aria-label', 'Processing pasted image')
  dom.setAttribute('role', 'status')
  dom.textContent = 'Processing image…'
  return { dom }
}

export function resizeSelectedImage(delta: number) {
  return (
    state: EditorView['state'],
    dispatch?: EditorView['dispatch'],
  ): boolean => {
    if (!(state.selection instanceof NodeSelection)) {
      return false
    }
    const node = state.selection.node
    if (node.type.name !== 'dara_image') {
      return false
    }
    const displayWidthPercent = clampImageDisplayWidth(
      Number(node.attrs.displayWidthPercent) + delta,
    )
    if (displayWidthPercent === node.attrs.displayWidthPercent) {
      return true
    }
    dispatch?.(
      state.tr.setNodeMarkup(state.selection.from, undefined, {
        ...node.attrs,
        displayWidthPercent,
      }),
    )
    return true
  }
}
