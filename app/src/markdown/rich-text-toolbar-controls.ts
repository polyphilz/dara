export const RichTextToolbarControl = {
  BlockQuote: 'BLOCK_QUOTE',
  Bold: 'BOLD',
  BulletedList: 'BULLETED_LIST',
  CodeBlock: 'CODE_BLOCK',
  DecreaseIndent: 'DECREASE_INDENT',
  DisplayMath: 'DISPLAY_MATH',
  Image: 'IMAGE',
  IncreaseIndent: 'INCREASE_INDENT',
  InlineCode: 'INLINE_CODE',
  InlineMath: 'INLINE_MATH',
  Italic: 'ITALIC',
  Link: 'LINK',
  NumberedList: 'NUMBERED_LIST',
  Redo: 'REDO',
  Strikethrough: 'STRIKETHROUGH',
  Undo: 'UNDO',
} as const

export type RichTextToolbarControl =
  (typeof RichTextToolbarControl)[keyof typeof RichTextToolbarControl]
