import {
  baseKeymap,
  chainCommands,
  createParagraphNear,
  liftEmptyBlock,
  newlineInCode,
  setBlockType,
  splitBlock,
  toggleMark,
  wrapIn,
} from 'prosemirror-commands'
import { dropCursor } from 'prosemirror-dropcursor'
import { gapCursor } from 'prosemirror-gapcursor'
import { history, redo, undo } from 'prosemirror-history'
import {
  InputRule,
  inputRules,
  textblockTypeInputRule,
  undoInputRule,
  wrappingInputRule,
} from 'prosemirror-inputrules'
import { keymap } from 'prosemirror-keymap'
import type { Node as ProseMirrorNode } from 'prosemirror-model'
import {
  liftListItem,
  sinkListItem,
  splitListItem,
  wrapInList,
} from 'prosemirror-schema-list'
import {
  EditorState,
  NodeSelection,
  Selection,
  type Command,
} from 'prosemirror-state'
import { tableEditing } from 'prosemirror-tables'
import { insertPoint } from 'prosemirror-transform'
import {
  EditorView,
  type DirectEditorProps,
  type NodeViewConstructor,
  type NodeView,
} from 'prosemirror-view'
import katex from 'katex'
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useReducer,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from 'react'
import { DaraButton } from '../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../components/dara-button-types.ts'
import { DaraInput } from '../components/DaraInput.tsx'
import { DaraText } from '../components/DaraText.tsx'
import {
  DaraTextTone,
  DaraTextVariant,
} from '../components/dara-text-types.ts'
import { DARA_WRITING_ASSISTANCE_ATTRIBUTES } from '../components/writing-assistance.ts'
import {
  ImageDisplayWidthStep,
  initialImageDisplayWidth,
  type ImageRecord,
} from '../media/image-reference.ts'
import { daraEditorSchema } from './editor-schema.ts'
import {
  imageNodeView,
  pendingImageNodeView,
  resizeSelectedImage,
} from './ImageNodeView.ts'
import {
  registerRichTextEditorView,
  unregisterRichTextEditorView,
} from './editor-view-registry.ts'
import {
  parseDaraMarkdown,
  serializeDaraMarkdown,
} from './markdown-conversion.ts'
import { externalHttpUrl } from './url-policy.ts'
import { CodeLanguageMenu } from './CodeLanguageMenu.tsx'

export interface RichTextEditorHandle {
  focus: () => void
}

interface RichTextEditorProps {
  ariaLabel: string
  disabled?: boolean
  ingestImage?: () => Promise<ImageRecord>
  onChange: (value: string) => void
  onMediaError?: (error: unknown) => void
  onPendingMediaChange?: (pending: boolean) => void
  placeholder?: string
  value: string
}

interface MathDialogState {
  display: boolean
  formula: string
  position?: number
}

interface LinkDialogState {
  href: string
}

const externalValueUpdate = 'dara-external-value-update'
let codeBlockNodeViewPromise: Promise<NodeViewConstructor> | null = null
let pendingImageRequestSequence = 0
const unavailableImageIngestion = () =>
  Promise.reject(new Error('Image ingestion requires an active editor lease.'))

export const RichTextEditor = forwardRef<
  RichTextEditorHandle,
  RichTextEditorProps
>(function RichTextEditor(
  {
    ariaLabel,
    disabled = false,
    ingestImage = unavailableImageIngestion,
    onChange,
    onMediaError,
    onPendingMediaChange,
    placeholder,
    value,
  },
  ref,
) {
  const hostRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const onChangeRef = useRef(onChange)
  const ingestImageRef = useRef(ingestImage)
  const onMediaErrorRef = useRef(onMediaError)
  const onPendingMediaChangeRef = useRef(onPendingMediaChange)
  const disabledRef = useRef(disabled)
  const openMathDialogRef = useRef(
    (_display: boolean, _formula: string, _position?: number) => undefined,
  )
  const openLinkDialogRef = useRef((_view: EditorView) => undefined)
  const initialValueRef = useRef(value)
  const initialAriaLabelRef = useRef(ariaLabel)
  const initialPlaceholderRef = useRef(placeholder)
  const [, renderToolbar] = useReducer((count) => count + 1, 0)
  const [mathDialog, setMathDialog] = useState<MathDialogState | null>(null)
  const [linkDialog, setLinkDialog] = useState<LinkDialogState | null>(null)

  onChangeRef.current = onChange
  ingestImageRef.current = ingestImage
  onMediaErrorRef.current = onMediaError
  onPendingMediaChangeRef.current = onPendingMediaChange
  disabledRef.current = disabled
  openMathDialogRef.current = (display, formula, position) => {
    setLinkDialog(null)
    setMathDialog({ display, formula, position })
  }
  openLinkDialogRef.current = (editorView) => {
    setMathDialog(null)
    setLinkDialog({ href: activeLinkHref(editorView) ?? 'https://' })
  }

  useImperativeHandle(
    ref,
    () => ({ focus: () => viewRef.current?.focus() }),
    [],
  )

  useLayoutEffect(() => {
    const host = hostRef.current
    if (!host) {
      return
    }

    const state = EditorState.create({
      doc: parseDaraMarkdown(initialValueRef.current, daraEditorSchema),
      plugins: [
        history(),
        editorInputRules(),
        keymap(editorKeyBindings((editorView) => openLinkDialogRef.current(editorView))),
        keymap(baseKeymap),
        dropCursor(),
        gapCursor(),
        tableEditing(),
      ],
      schema: daraEditorSchema,
    })

    const props: DirectEditorProps = {
      attributes: {
        ...DARA_WRITING_ASSISTANCE_ATTRIBUTES,
        'aria-label': initialAriaLabelRef.current,
        'aria-multiline': 'true',
        class: 'dara-rich-text-content',
        role: 'textbox',
      },
      dispatchTransaction(transaction) {
        const view = viewRef.current
        if (!view) {
          return
        }
        const nextState = view.state.apply(transaction)
        view.updateState(nextState)
        updateEmptyState(view)
        notifyPendingMedia(nextState.doc, onPendingMediaChangeRef.current)
        void installCodeBlockNodeView(view)
        renderToolbar()
        if (
          transaction.docChanged &&
          transaction.getMeta(externalValueUpdate) !== true
        ) {
          onChangeRef.current(serializeDaraMarkdown(nextState.doc))
        }
      },
      editable: () => !disabledRef.current,
      handleDOMEvents: {
        paste(editorView, event) {
          const clipboardEvent = event as ClipboardEvent
          if (
            disabledRef.current ||
            !clipboardContainsImage(clipboardEvent.clipboardData)
          ) {
            return false
          }
          clipboardEvent.preventDefault()
          beginImagePaste(
            editorView,
            ingestImageRef.current,
            onMediaErrorRef,
            viewRef,
          )
          return true
        },
      },
      nodeViews: {
        dara_image: imageNodeView,
        dara_image_pending: pendingImageNodeView,
        math_display: (node, editorView, getPos) =>
          mathNodeView(
            node,
            editorView,
            getPos,
            openMathDialogRef.current,
          ),
        math_inline: (node, editorView, getPos) =>
          mathNodeView(
            node,
            editorView,
            getPos,
            openMathDialogRef.current,
          ),
      },
      state,
    }

    const view = new EditorView(host, props)
    viewRef.current = view
    registerRichTextEditorView(view)
    view.dom.dataset.placeholder = initialPlaceholderRef.current ?? ''
    updateEmptyState(view)
    notifyPendingMedia(view.state.doc, onPendingMediaChangeRef.current)
    void installCodeBlockNodeView(view)
    renderToolbar()

    return () => {
      onPendingMediaChangeRef.current?.(false)
      viewRef.current = null
      unregisterRichTextEditorView(view)
      view.destroy()
    }
  }, [])

  useEffect(() => {
    const view = viewRef.current
    if (!view) {
      return
    }
    view.setProps({ editable: () => !disabled })
    view.dom.setAttribute('aria-disabled', String(disabled))
    renderToolbar()
  }, [disabled])

  useEffect(() => {
    const view = viewRef.current
    if (!view) {
      return
    }
    const current = serializeDaraMarkdown(view.state.doc)
    if (current === value) {
      return
    }
    const replacement = parseDaraMarkdown(value, daraEditorSchema)
    if (view.state.doc.eq(replacement)) {
      return
    }
    const transaction = view.state.tr
      .replaceWith(0, view.state.doc.content.size, replacement.content)
      .setMeta(externalValueUpdate, true)
      .setMeta('addToHistory', false)
    view.dispatch(transaction)
  }, [value])

  const view = viewRef.current
  return (
    <div
      aria-disabled={disabled || undefined}
      className="rich-text-editor"
      data-testid={`${ariaLabel.toLowerCase().replace(/\s+/g, '-')}-editor`}
    >
      <EditorToolbar
        ariaLabel={ariaLabel}
        disabled={disabled}
        onLink={() => view && openLinkDialogRef.current(view)}
        onMath={(display) => {
          setLinkDialog(null)
          setMathDialog({ display, formula: '' })
        }}
        view={view}
      />
      <div className="rich-text-editor-surface" ref={hostRef} />
      {mathDialog && view && (
        <FormulaDialog
          dialog={mathDialog}
          onCancel={() => {
            setMathDialog(null)
            view.focus()
          }}
          onConfirm={(formula) => {
            commitMath(view, mathDialog, formula)
            setMathDialog(null)
          }}
        />
      )}
      {linkDialog && view && (
        <LinkDialog
          dialog={linkDialog}
          onCancel={() => {
            setLinkDialog(null)
            view.focus()
          }}
          onConfirm={(href) => {
            applyLink(view, href)
            setLinkDialog(null)
          }}
        />
      )}
    </div>
  )
})

function clipboardContainsImage(data: DataTransfer | null): boolean {
  if (!data) {
    return false
  }
  return Array.from(data.items).some((item) => item.type.startsWith('image/'))
}

function beginImagePaste(
  view: EditorView,
  ingestImage: () => Promise<ImageRecord>,
  onMediaErrorRef: { current: ((error: unknown) => void) | undefined },
  viewRef: { current: EditorView | null },
) {
  pendingImageRequestSequence += 1
  const requestId = `image-paste-${pendingImageRequestSequence}`
  const pendingNode = daraEditorSchema.nodes.dara_image_pending!.create({
    requestId,
  })
  view.dispatch(
    view.state.tr.replaceSelectionWith(pendingNode, false).scrollIntoView(),
  )

  void ingestImage().then(
    (record) => {
      if (viewRef.current !== view) {
        return
      }
      const position = pendingImagePosition(view.state.doc, requestId)
      if (position === null) {
        return
      }
      const imageNode = daraEditorSchema.nodes.dara_image!.create({
        displayWidthPercent: initialImageDisplayWidth(
          record.naturalWidth,
          view.dom.clientWidth,
        ),
        imageId: record.id,
      })
      const transaction = view.state.tr.replaceWith(
        position,
        position + view.state.doc.nodeAt(position)!.nodeSize,
        imageNode,
      )
      transaction.setSelection(NodeSelection.create(transaction.doc, position))
      view.dispatch(transaction.scrollIntoView())
    },
    (error: unknown) => {
      if (viewRef.current === view) {
        const position = pendingImagePosition(view.state.doc, requestId)
        if (position !== null) {
          const node = view.state.doc.nodeAt(position)
          if (node) {
            view.dispatch(view.state.tr.delete(position, position + node.nodeSize))
          }
        }
      }
      onMediaErrorRef.current?.(error)
    },
  )
}

function pendingImagePosition(
  document: ProseMirrorNode,
  requestId: string,
): number | null {
  let position: number | null = null
  document.descendants((node, nodePosition) => {
    if (
      node.type.name === 'dara_image_pending' &&
      node.attrs.requestId === requestId
    ) {
      position = nodePosition
      return false
    }
    return position === null
  })
  return position
}

function notifyPendingMedia(
  document: ProseMirrorNode,
  callback: ((pending: boolean) => void) | undefined,
) {
  if (!callback) {
    return
  }
  let pending = false
  document.descendants((node) => {
    if (node.type.name === 'dara_image_pending') {
      pending = true
      return false
    }
    return !pending
  })
  callback(pending)
}

function editorKeyBindings(openLink: (view: EditorView) => void): Record<string, Command> {
  const listItem = daraEditorSchema.nodes.list_item!
  const hardBreak = insertHardBreak()
  return {
    'Alt-F10': focusEditorToolbar,
    Backspace: undoInputRule,
    'Mod-[': liftListItem(listItem),
    'Mod-]': sinkListItem(listItem),
    'Mod-b': toggleMark(daraEditorSchema.marks.strong!),
    'Mod-e': toggleMark(daraEditorSchema.marks.code!),
    'Mod-i': toggleMark(daraEditorSchema.marks.em!),
    'Mod-k': (_state, _dispatch, view) => {
      if (!view) {
        return false
      }
      openLink(view)
      return true
    },
    'Mod-Shift-s': toggleMark(daraEditorSchema.marks.strike!),
    'Mod-Shift-x': toggleMark(daraEditorSchema.marks.strike!),
    'Alt-ArrowLeft': resizeSelectedImage(-ImageDisplayWidthStep),
    'Alt-ArrowRight': resizeSelectedImage(ImageDisplayWidthStep),
    'Mod-z': undo,
    'Mod-y': redo,
    'Mod-Shift-z': redo,
    Enter: chainCommands(
      splitListItem(listItem),
      newlineInCode,
      createParagraphNear,
      liftEmptyBlock,
      splitBlock,
    ),
    'Shift-Enter': hardBreak,
    ArrowDown: arrowIntoCodeBlock('down'),
    ArrowLeft: arrowIntoCodeBlock('left'),
    ArrowRight: arrowIntoCodeBlock('right'),
    ArrowUp: arrowIntoCodeBlock('up'),
  }
}

const focusEditorToolbar: Command = (_state, _dispatch, view) => {
  const toolbar = view?.dom
    .closest('.rich-text-editor')
    ?.querySelector<HTMLElement>('.rich-text-toolbar')
  if (!toolbar) {
    return false
  }
  toolbar.focus()
  return true
}

function editorInputRules() {
  const bulletList = daraEditorSchema.nodes.bullet_list!
  const codeBlock = daraEditorSchema.nodes.code_block!
  const orderedList = daraEditorSchema.nodes.ordered_list!
  return inputRules({
    rules: [
      textblockTypeInputRule(/^```$/, codeBlock, { language: null }),
      inlineCodeInputRule(),
      wrappingInputRule(/^[-*•]\s$/, bulletList),
      wrappingInputRule(
        /^(\d+)\.\s$/,
        orderedList,
        (match) => ({ order: Number(match[1]) }),
        (match, node) =>
          node.childCount + Number(node.attrs.order) === Number(match[1]),
      ),
    ],
  })
}

function inlineCodeInputRule(): InputRule {
  return new InputRule(
    /`([^`\n]+)`$/,
    (state, _match, start, end) => {
      const code = state.schema.marks.code
      if (!code) {
        return null
      }
      const tr = state.tr
        .delete(start, start + 1)
        .addMark(start, end - 1, code.create())
      // Closing the backtick leaves the caret just inside the new mark, so
      // clear it from the stored marks and let typing continue as plain text.
      return tr.removeStoredMark(code)
    },
    { inCodeMark: false },
  )
}

function arrowIntoCodeBlock(
  direction: 'down' | 'left' | 'right' | 'up',
): Command {
  return (state, dispatch, view) => {
    if (!view || !state.selection.empty || !view.endOfTextblock(direction)) {
      return false
    }
    const side = direction === 'left' || direction === 'up' ? -1 : 1
    const { $head } = state.selection
    if ($head.depth === 0) {
      return false
    }
    const next = Selection.near(
      state.doc.resolve(side > 0 ? $head.after() : $head.before()),
      side,
    )
    if (next.$head.parent.type.name !== 'code_block') {
      return false
    }
    dispatch?.(state.tr.setSelection(next))
    return true
  }
}

function insertHardBreak(): Command {
  return (state, dispatch) => {
    const hardBreak = state.schema.nodes.hard_break
    if (!hardBreak) {
      return false
    }
    if (dispatch) {
      dispatch(state.tr.replaceSelectionWith(hardBreak.create()).scrollIntoView())
    }
    return true
  }
}

function EditorToolbar({
  ariaLabel,
  disabled,
  onLink,
  onMath,
  view,
}: {
  ariaLabel: string
  disabled: boolean
  onLink: () => void
  onMath: (display: boolean) => void
  view: EditorView | null
}) {
  const commandButton = (
    label: string,
    shortLabel: ReactNode,
    command: Command,
    active = false,
    shortcut?: string,
  ) => (
    <ToolbarButton
      active={active}
      disabled={disabled || !view || !command(view.state)}
      key={label}
      label={label}
      onPress={() => view && runCommand(view, command)}
      shortcut={shortcut}
    >
      {shortLabel}
    </ToolbarButton>
  )

  return (
    <div
      aria-label={`${ariaLabel} formatting`}
      aria-keyshortcuts="Alt+F10"
      className="rich-text-toolbar"
      onKeyDown={handleToolbarKeys}
      role="toolbar"
      tabIndex={-1}
    >
      {commandButton(
        'Bold',
        'B',
        toggleMark(daraEditorSchema.marks.strong!),
        markIsActive(view, 'strong'),
        '⌘B',
      )}
      {commandButton(
        'Italic',
        'I',
        toggleMark(daraEditorSchema.marks.em!),
        markIsActive(view, 'em'),
        '⌘I',
      )}
      {commandButton(
        'Strikethrough',
        <span className="toolbar-strike">S</span>,
        toggleMark(daraEditorSchema.marks.strike!),
        markIsActive(view, 'strike'),
        '⇧⌘S',
      )}
      <ToolbarButton
        active={markIsActive(view, 'link')}
        disabled={disabled || !view}
        label="Link"
        onPress={onLink}
        shortcut="⌘K"
      >
        <LinkIcon />
      </ToolbarButton>
      <span aria-hidden="true" className="toolbar-divider" />
      {commandButton(
        'Bulleted list',
        <ListIcon variant="bulleted" />,
        toggleList('bullet_list'),
        blockIsActive(view, 'bullet_list'),
      )}
      {commandButton(
        'Numbered list',
        <ListIcon variant="numbered" />,
        toggleList('ordered_list'),
        blockIsActive(view, 'ordered_list'),
      )}
      {commandButton(
        'Decrease indent',
        <IndentIcon direction="decrease" />,
        liftListItem(daraEditorSchema.nodes.list_item!),
        false,
        '⌘[',
      )}
      {commandButton(
        'Increase indent',
        <IndentIcon direction="increase" />,
        sinkListItem(daraEditorSchema.nodes.list_item!),
        false,
        '⌘]',
      )}
      <span aria-hidden="true" className="toolbar-divider" />
      {commandButton(
        'Block quote',
        <QuoteIcon />,
        toggleBlock('blockquote'),
        blockIsActive(view, 'blockquote'),
      )}
      {commandButton(
        'Inline code',
        <InlineCodeIcon />,
        toggleMark(daraEditorSchema.marks.code!),
        markIsActive(view, 'code'),
        '⌘E',
      )}
      {commandButton(
        'Code block',
        <CodeBlockIcon />,
        toggleTextBlock('code_block'),
        blockIsActive(view, 'code_block'),
      )}
      <CodeLanguageControl disabled={disabled} view={view} />
      <ToolbarButton
        disabled={disabled || !view}
        label="Inline math"
        onPress={() => onMath(false)}
      >
        <span className="toolbar-math">fx</span>
      </ToolbarButton>
      <ToolbarButton
        disabled={disabled || !view}
        label="Display math"
        onPress={() => onMath(true)}
      >
        <span className="toolbar-math-symbol">∑</span>
      </ToolbarButton>
      <span aria-hidden="true" className="toolbar-divider" />
      {commandButton('Undo', <HistoryIcon direction="undo" />, undo, false, '⌘Z')}
      {commandButton('Redo', <HistoryIcon direction="redo" />, redo, false, '⇧⌘Z')}
    </div>
  )
}

function CodeLanguageControl({
  disabled,
  view,
}: {
  disabled: boolean
  view: EditorView | null
}) {
  const language = activeCodeLanguage(view)
  if (language === undefined) {
    return null
  }
  return (
    <CodeLanguageMenu
      disabled={disabled}
      language={language}
      onReturnFocus={() => view?.focus()}
      onSelect={(nextLanguage) => {
        if (view) {
          setCodeLanguage(view, nextLanguage)
        }
      }}
    />
  )
}

/**
 * The familiar indent/outdent glyph: stacked text lines whose middle rows are
 * indented, with a chevron pointing the way the indent moves.
 */
function IndentIcon({ direction }: { direction: 'decrease' | 'increase' }) {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <path d="M4 5h16" />
      <path d="M11 10h9" />
      <path d="M11 14h9" />
      <path d="M4 19h16" />
      <path
        d={direction === 'increase' ? 'm4 9 3.5 3-3.5 3' : 'm7.5 9-3.5 3 3.5 3'}
      />
    </svg>
  )
}

/** The familiar list glyphs: markers down the left, text rules to the right. */
function ListIcon({ variant }: { variant: 'bulleted' | 'numbered' }) {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <path d="M10 5h10" />
      <path d="M10 12h10" />
      <path d="M10 19h10" />
      {variant === 'bulleted' ? (
        <>
          <circle className="toolbar-icon-dot" cx="4.5" cy="5" r="1.5" />
          <circle className="toolbar-icon-dot" cx="4.5" cy="12" r="1.5" />
          <circle className="toolbar-icon-dot" cx="4.5" cy="19" r="1.5" />
        </>
      ) : (
        <>
          <path className="toolbar-icon-numeral" d="m3.5 3.6 1.3-.9v4.6" />
          <path
            className="toolbar-icon-numeral"
            d="M3.5 10.5c.3-.8 1.9-.8 1.9.3 0 1-1.9 1.7-1.9 3h2.1"
          />
          <path
            className="toolbar-icon-numeral"
            d="M3.5 17.1c.4-.6 1.9-.6 1.9.4 0 .6-.6.8-1.1.8.6 0 1.2.3 1.2 1 0 1-1.5 1.2-2 .5"
          />
        </>
      )}
    </svg>
  )
}

/** A curved arrow, doubling back the way the history step travels. */
function HistoryIcon({ direction }: { direction: 'redo' | 'undo' }) {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      {direction === 'undo' ? (
        <>
          <path d="M4.5 10h9.5a4.75 4.75 0 1 1 0 9.5H9" />
          <path d="m8.5 5.5-4 4.5 4 4.5" />
        </>
      ) : (
        <>
          <path d="M19.5 10H10a4.75 4.75 0 1 0 0 9.5H15" />
          <path d="m15.5 5.5 4 4.5-4 4.5" />
        </>
      )}
    </svg>
  )
}

/** A horizontal chain link, the near-universal mark for inserting a link. */
function LinkIcon() {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <path d="M10.5 8.5H7.8a3.5 3.5 0 0 0 0 7h2.7" />
      <path d="M13.5 8.5h2.7a3.5 3.5 0 0 1 0 7h-2.7" />
      <path d="M8.8 12h6.4" />
    </svg>
  )
}

/** Opening double quotation marks: a round bowl with a tail sweeping up. */
function QuoteIcon() {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <circle className="toolbar-icon-solid" cx="7.2" cy="14.4" r="3.2" />
      <circle className="toolbar-icon-solid" cx="16.4" cy="14.4" r="3.2" />
      <path className="toolbar-icon-tail" d="M5 12.8q0-5.2 3.5-6.9" />
      <path className="toolbar-icon-tail" d="M14.2 12.8q0-5.2 3.5-6.9" />
    </svg>
  )
}

/** Bare angle brackets: code that lives inside a line of prose. */
function InlineCodeIcon() {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <path d="m8 8.5-3.5 3.5 3.5 3.5" />
      <path d="m16 8.5 3.5 3.5-3.5 3.5" />
      <path d="m13.4 7-2.8 10" />
    </svg>
  )
}

/** The same brackets, framed: code that owns its own block. */
function CodeBlockIcon() {
  return (
    <svg
      aria-hidden="true"
      className="toolbar-icon"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <rect height="16" rx="2.5" width="19" x="2.5" y="4" />
      <path className="toolbar-icon-hairline" d="m9.6 9.4-2.6 2.6 2.6 2.6" />
      <path className="toolbar-icon-hairline" d="m14.4 9.4 2.6 2.6-2.6 2.6" />
    </svg>
  )
}

function ToolbarButton({
  active = false,
  children,
  disabled,
  label,
  onPress,
  shortcut,
}: {
  active?: boolean
  children: ReactNode
  disabled: boolean
  label: string
  onPress: () => void
  shortcut?: string
}) {
  const handleMouseDown = (event: MouseEvent<HTMLButtonElement>) => {
    event.preventDefault()
    onPress()
  }
  return (
    <DaraButton
      aria-keyshortcuts={shortcut}
      aria-label={label}
      aria-pressed={active}
      className={active ? 'toolbar-button toolbar-button-active' : 'toolbar-button'}
      disabled={disabled}
      onMouseDown={handleMouseDown}
      size={DaraButtonSize.Custom}
      tabIndex={-1}
      title={shortcut ? `${label} (${shortcut})` : label}
      type="button"
      variant={DaraButtonVariant.Custom}
    >
      {children}
    </DaraButton>
  )
}

function handleToolbarKeys(event: KeyboardEvent<HTMLDivElement>) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
    return
  }
  const controls = Array.from(
    event.currentTarget.querySelectorAll<HTMLElement>(
      'button:not(:disabled), select:not(:disabled)',
    ),
  )
  if (!controls.length) {
    return
  }
  event.preventDefault()
  const current = controls.indexOf(document.activeElement as HTMLElement)
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? controls.length - 1
        : event.key === 'ArrowRight'
          ? (current + 1 + controls.length) % controls.length
          : (current - 1 + controls.length) % controls.length
  controls[next]?.focus()
}

function runCommand(view: EditorView, command: Command): boolean {
  const applied = command(view.state, view.dispatch, view)
  if (applied) {
    view.focus()
  }
  return applied
}

function markIsActive(view: EditorView | null, markName: string): boolean {
  if (!view) {
    return false
  }
  const type = view.state.schema.marks[markName]
  if (!type) {
    return false
  }
  const { empty, from, $from, to } = view.state.selection
  return empty
    ? Boolean(type.isInSet(view.state.storedMarks ?? $from.marks()))
    : view.state.doc.rangeHasMark(from, to, type)
}

function blockIsActive(view: EditorView | null, nodeName: string): boolean {
  if (!view) {
    return false
  }
  const { $from } = view.state.selection
  for (let depth = $from.depth; depth >= 0; depth -= 1) {
    if ($from.node(depth).type.name === nodeName) {
      return true
    }
  }
  return false
}

function activeCodeLanguage(view: EditorView | null): string | null | undefined {
  if (!view) {
    return undefined
  }
  const { $from } = view.state.selection
  for (let depth = $from.depth; depth >= 0; depth -= 1) {
    const node = $from.node(depth)
    if (node.type.name === 'code_block') {
      return node.attrs.language ?? null
    }
  }
  return undefined
}

function setCodeLanguage(view: EditorView, language: string | null): void {
  const { $from } = view.state.selection
  for (let depth = $from.depth; depth >= 1; depth -= 1) {
    const node = $from.node(depth)
    if (node.type.name === 'code_block') {
      view.dispatch(
        view.state.tr.setNodeMarkup($from.before(depth), undefined, {
          ...node.attrs,
          language,
        }),
      )
      view.focus()
      return
    }
  }
}

function toggleList(nodeName: 'bullet_list' | 'ordered_list'): Command {
  return (state, dispatch) => {
    const list = state.schema.nodes[nodeName]!
    const item = state.schema.nodes.list_item!
    const inList = ancestorIsActive(state, nodeName)
    return inList
      ? liftListItem(item)(state, dispatch)
      : wrapInList(list)(state, dispatch)
  }
}

function toggleBlock(nodeName: 'blockquote'): Command {
  return (state, dispatch) => {
    const type = state.schema.nodes[nodeName]!
    if (ancestorIsActive(state, nodeName)) {
      const { $from, $to } = state.selection
      const range = $from.blockRange($to)
      const target = range ? 0 : null
      if (!range || target === null) {
        return false
      }
      if (dispatch) {
        dispatch(state.tr.lift(range, target).scrollIntoView())
      }
      return true
    }
    return wrapIn(type)(state, dispatch)
  }
}

function toggleTextBlock(nodeName: 'code_block'): Command {
  return (state, dispatch) =>
    setBlockType(
      ancestorIsActive(state, nodeName)
        ? state.schema.nodes.paragraph!
        : state.schema.nodes[nodeName]!,
    )(state, dispatch)
}

function ancestorIsActive(state: EditorState, nodeName: string): boolean {
  const { $from } = state.selection
  for (let depth = $from.depth; depth >= 0; depth -= 1) {
    if ($from.node(depth).type.name === nodeName) {
      return true
    }
  }
  return false
}

function activeLinkHref(view: EditorView): string | null {
  const current = view.state.schema.marks.link!.isInSet(
    view.state.storedMarks ?? view.state.selection.$from.marks(),
  )
  return typeof current?.attrs.href === 'string' ? current.attrs.href : null
}

function applyLink(view: EditorView, href: string | null): void {
  const link = view.state.schema.marks.link!
  if (href === null) {
    if (markIsActive(view, 'link')) {
      runCommand(view, toggleMark(link))
    } else {
      view.focus()
    }
    return
  }
  runCommand(view, toggleMark(link, { href, title: null }))
}

function commitMath(
  view: EditorView,
  dialog: MathDialogState,
  formula: string,
): void {
  if (dialog.position !== undefined) {
    const node = view.state.doc.nodeAt(dialog.position)
    if (node && ['math_inline', 'math_display'].includes(node.type.name)) {
      view.dispatch(
        view.state.tr.setNodeMarkup(dialog.position, undefined, { formula }),
      )
    }
    view.focus()
    return
  }
  const type = dialog.display
    ? view.state.schema.nodes.math_display!
    : view.state.schema.nodes.math_inline!
  let transaction = view.state.tr
  if (dialog.display) {
    const position =
      insertPoint(view.state.doc, view.state.selection.to, type) ??
      insertPoint(view.state.doc, view.state.selection.from, type)
    if (position === null) {
      view.focus()
      return
    }
    transaction = transaction.insert(position, type.create({ formula }))
    transaction = transaction.setSelection(
      NodeSelection.create(transaction.doc, position),
    )
  } else {
    transaction = transaction.replaceSelectionWith(type.create({ formula }))
  }
  view.dispatch(transaction.scrollIntoView())
  view.focus()
}

function FormulaDialog({
  dialog,
  onCancel,
  onConfirm,
}: {
  dialog: MathDialogState
  onCancel: () => void
  onConfirm: (formula: string) => void
}) {
  const [formula, setFormula] = useState(dialog.formula)
  const inputRef = useRef<HTMLInputElement>(null)
  const previewRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [])

  useEffect(() => {
    if (!previewRef.current) {
      return
    }
    katex.render(formula || '\\square', previewRef.current, {
      displayMode: dialog.display,
      maxExpand: 1_000,
      maxSize: 20,
      output: 'htmlAndMathml',
      strict: 'warn',
      throwOnError: false,
      trust: false,
    })
  }, [dialog.display, formula])

  const insertSnippet = (snippet: string, cursorOffset = snippet.length) => {
    const input = inputRef.current
    if (!input) {
      return
    }
    const start = input.selectionStart ?? formula.length
    const end = input.selectionEnd ?? start
    const next = formula.slice(0, start) + snippet + formula.slice(end)
    setFormula(next)
    requestAnimationFrame(() => {
      input.focus()
      const cursor = start + cursorOffset
      input.setSelectionRange(cursor, cursor)
    })
  }

  return (
    <form
      aria-label={dialog.display ? 'Display math editor' : 'Inline math editor'}
      className="formula-dialog"
      onKeyDown={(event) => {
        event.stopPropagation()
        if (event.key === 'Escape') {
          event.preventDefault()
          onCancel()
        }
      }}
      onSubmit={(event) => {
        event.preventDefault()
        if (formula.trim()) {
          onConfirm(formula)
        }
      }}
      role="dialog"
    >
      <div className="formula-dialog-heading">
        <DaraText as="strong" variant={DaraTextVariant.Label}>
          {dialog.display ? 'Display equation' : 'Inline equation'}
        </DaraText>
        <DaraText
          as="span"
          tone={DaraTextTone.Muted}
          variant={DaraTextVariant.Caption}
        >
          LaTeX-style formula
        </DaraText>
      </div>
      <div aria-live="polite" className="formula-preview" ref={previewRef} />
      <DaraInput
        aria-label="Formula"
        onChange={(event) => setFormula(event.target.value)}
        ref={inputRef}
        type="text"
        value={formula}
      />
      <div aria-label="Math symbols" className="formula-symbols" role="toolbar">
        <DaraButton
          onClick={() => insertSnippet('\\pi')}
          size={DaraButtonSize.Compact}
          variant={DaraButtonVariant.Ghost}
        >
          π
        </DaraButton>
        <DaraButton
          onClick={() => insertSnippet('\\sqrt{}', 6)}
          size={DaraButtonSize.Compact}
          variant={DaraButtonVariant.Ghost}
        >
          √
        </DaraButton>
        <DaraButton
          onClick={() => insertSnippet('^{}', 2)}
          size={DaraButtonSize.Compact}
          variant={DaraButtonVariant.Ghost}
        >
          x²
        </DaraButton>
        <DaraButton
          onClick={() => insertSnippet('\\frac{}{}', 6)}
          size={DaraButtonSize.Compact}
          variant={DaraButtonVariant.Ghost}
        >
          a⁄b
        </DaraButton>
        <DaraButton
          onClick={() => insertSnippet('\\sum_{}^{}', 6)}
          size={DaraButtonSize.Compact}
          variant={DaraButtonVariant.Ghost}
        >
          ∑
        </DaraButton>
        <DaraButton
          onClick={() => insertSnippet('\\infty')}
          size={DaraButtonSize.Compact}
          variant={DaraButtonVariant.Ghost}
        >
          ∞
        </DaraButton>
      </div>
      <div className="formula-dialog-actions">
        <DaraButton onClick={onCancel} variant={DaraButtonVariant.Ghost}>
          Cancel
        </DaraButton>
        <DaraButton
          disabled={!formula.trim()}
          type="submit"
          variant={DaraButtonVariant.Primary}
        >
          Apply
        </DaraButton>
      </div>
    </form>
  )
}

function LinkDialog({
  dialog,
  onCancel,
  onConfirm,
}: {
  dialog: LinkDialogState
  onCancel: () => void
  onConfirm: (href: string | null) => void
}) {
  const [href, setHref] = useState(dialog.href)
  const [error, setError] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [])

  return (
    <form
      aria-label="Link editor"
      className="formula-dialog"
      onKeyDown={(event) => {
        event.stopPropagation()
        if (event.key === 'Escape') {
          event.preventDefault()
          onCancel()
        }
      }}
      onSubmit={(event) => {
        event.preventDefault()
        const entered = href.trim()
        if (!entered) {
          onConfirm(null)
          return
        }
        const safeHref = externalHttpUrl(entered)
        if (!safeHref) {
          setError('Links must use http:// or https://.')
          return
        }
        onConfirm(safeHref)
      }}
      role="dialog"
    >
      <div className="formula-dialog-heading">
        <DaraText as="strong" variant={DaraTextVariant.Label}>
          Link
        </DaraText>
        <DaraText
          as="span"
          tone={DaraTextTone.Muted}
          variant={DaraTextVariant.Caption}
        >
          Absolute HTTP URL
        </DaraText>
      </div>
      <DaraInput
        aria-label="Link URL"
        onChange={(event) => {
          setHref(event.target.value)
          setError(null)
        }}
        ref={inputRef}
        type="url"
        value={href}
      />
      {error && <span role="alert">{error}</span>}
      <div className="formula-dialog-actions">
        <DaraButton onClick={onCancel} variant={DaraButtonVariant.Ghost}>
          Cancel
        </DaraButton>
        <DaraButton type="submit" variant={DaraButtonVariant.Primary}>
          Apply
        </DaraButton>
      </div>
    </form>
  )
}

function mathNodeView(
  node: ProseMirrorNode,
  _view: EditorView,
  getPos: () => number | undefined,
  openEditor: (display: boolean, formula: string, position?: number) => void,
): NodeView {
  return new MathNodeView(node, getPos, openEditor)
}

class MathNodeView implements NodeView {
  dom: HTMLElement
  private node: ProseMirrorNode
  private readonly getPos: () => number | undefined
  private readonly openEditor: (
    display: boolean,
    formula: string,
    position?: number,
  ) => void

  constructor(
    node: ProseMirrorNode,
    getPos: () => number | undefined,
    openEditor: (display: boolean, formula: string, position?: number) => void,
  ) {
    this.node = node
    this.getPos = getPos
    this.openEditor = openEditor
    this.dom = document.createElement(
      node.type.name === 'math_inline' ? 'span' : 'div',
    )
    this.dom.className = `dara-math-node ${
      node.type.name === 'math_inline'
        ? 'dara-math-inline'
        : 'dara-math-display'
    }`
    this.dom.contentEditable = 'false'
    this.dom.tabIndex = 0
    this.dom.addEventListener('dblclick', this.edit)
    this.dom.addEventListener('keydown', this.handleKeyDown)
    this.render()
  }

  update = (node: ProseMirrorNode): boolean => {
    if (node.type !== this.node.type) {
      return false
    }
    this.node = node
    this.render()
    return true
  }

  selectNode = () => this.dom.classList.add('dara-math-selected')
  deselectNode = () => this.dom.classList.remove('dara-math-selected')

  destroy = () => {
    this.dom.removeEventListener('dblclick', this.edit)
    this.dom.removeEventListener('keydown', this.handleKeyDown)
  }

  stopEvent = (event: Event) =>
    event.type === 'dblclick' || event.type === 'keydown'

  private handleKeyDown = (event: globalThis.KeyboardEvent) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      this.edit()
    }
  }

  private edit = () => {
    const position = this.getPos()
    if (position === undefined) {
      return
    }
    this.openEditor(
      this.node.type.name === 'math_display',
      this.node.attrs.formula,
      position,
    )
  }

  private render() {
    const formula = this.node.attrs.formula as string
    this.dom.dataset.formula = formula
    this.dom.setAttribute('aria-label', `Math: ${formula || 'empty formula'}`)
    katex.render(formula || '\\square', this.dom, {
      displayMode: this.node.type.name === 'math_display',
      maxExpand: 1_000,
      maxSize: 20,
      output: 'htmlAndMathml',
      strict: 'warn',
      throwOnError: false,
      trust: false,
    })
  }
}

function updateEmptyState(view: EditorView) {
  view.dom.dataset.empty = String(
    view.state.doc.childCount === 1 &&
      view.state.doc.firstChild?.type.name === 'paragraph' &&
      view.state.doc.firstChild.content.size === 0,
  )
}

async function installCodeBlockNodeView(view: EditorView): Promise<void> {
  if (view.props.nodeViews?.code_block || !documentHasCodeBlock(view.state.doc)) {
    return
  }
  codeBlockNodeViewPromise ??= import('./CodeBlockNodeView.ts').then(
    ({ codeBlockNodeView }) => codeBlockNodeView,
  )
  const constructor = await codeBlockNodeViewPromise
  if (view.isDestroyed || view.props.nodeViews?.code_block) {
    return
  }
  view.setProps({
    nodeViews: {
      ...view.props.nodeViews,
      code_block: constructor,
    },
  })
}

function documentHasCodeBlock(document: ProseMirrorNode): boolean {
  let found = false
  document.descendants((node) => {
    if (node.type.name === 'code_block') {
      found = true
      return false
    }
    return !found
  })
  return found
}
