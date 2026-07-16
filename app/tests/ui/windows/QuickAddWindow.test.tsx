import userEvent from '@testing-library/user-event'
import { EditorView as CodeMirrorView } from '@codemirror/view'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import { parseDaraMarkdown } from '../../../src/markdown/markdown-conversion.ts'
import { CardContentType } from '../../../src/review/contracts.ts'

const mocks = vi.hoisted(() => ({
  createCardContent: vi.fn(),
  dismissQuickAdd: vi.fn(),
  emit: vi.fn(),
  listen: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  emit: mocks.emit,
  listen: mocks.listen,
}))

vi.mock('../../../src/lib/native.ts', () => ({
  native: {
    dismissQuickAdd: mocks.dismissQuickAdd,
  },
}))

vi.mock('../../../src/review/index.ts', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../src/review/index.ts')>()),
  createCardContent: mocks.createCardContent,
  updateCardContent: vi.fn(),
}))

import { QuickAddWindow } from '../../../src/windows/quick-add/QuickAddWindow.tsx'

beforeEach(() => {
  vi.clearAllMocks()
  mocks.createCardContent.mockResolvedValue(undefined)
  mocks.dismissQuickAdd.mockResolvedValue(undefined)
  mocks.emit.mockResolvedValue(undefined)
  mocks.listen.mockResolvedValue(() => undefined)
})

test('focuses Front and follows the logical editor and form focus order', async () => {
  const user = userEvent.setup()
  const { getByRole, queryByRole } = render(<QuickAddWindow />)
  expect(getByRole('region', { name: 'Quick add' })).toBeTruthy()
  expect(getByRole('button', { name: 'Card type: Basic' })).toBeTruthy()
  expect(queryByRole('heading', { name: 'Quick add' })).toBeNull()
  const front = getByRole('textbox', { name: 'Front' })
  const back = getByRole('textbox', { name: 'Back' })
  const source = getByRole('textbox', { name: /Source/ })
  const add = getByRole('button', { name: /Add/ })

  await waitFor(() => expect(document.activeElement).toBe(front))
  await user.tab()
  expect(document.activeElement).toBe(back)
  await user.tab()
  expect(document.activeElement).toBe(source)
  await user.tab()
  expect(document.activeElement).toBe(add)
  await user.tab({ shift: true })
  expect(document.activeElement).toBe(source)
  await user.tab({ shift: true })
  expect(document.activeElement).toBe(back)
  await user.tab({ shift: true })
  expect(document.activeElement).toBe(front)
})

test('persists canonical Markdown, trims source, and clears all values after success', async () => {
  const { getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const back = getByRole('textbox', { name: 'Back' })
  const source = getByRole('textbox', { name: /Source/ })

  replaceEditorDocument(front, '**question**')
  replaceEditorDocument(back, 'answer\nwith a break')
  fireEvent.change(source, { target: { value: '  Chapter 4  ' } })
  fireEvent.click(getByRole('button', { name: /Add/ }))

  await waitFor(() => {
    expect(mocks.createCardContent).toHaveBeenCalledWith({
      backMd: 'answer\nwith a break',
      frontMd: '**question**',
      source: 'Chapter 4',
      type: CardContentType.Basic,
    })
  })
  await waitFor(() => {
    expect(editorView(front).state.doc.textContent).toBe('')
    expect(editorView(back).state.doc.textContent).toBe('')
    expect((source as HTMLInputElement).value).toBe('')
  })
  expect(mocks.emit).toHaveBeenCalledWith('card-created')
  expect(mocks.dismissQuickAdd).toHaveBeenCalledTimes(1)
})

test('normalizes whitespace-only source to null and Mod-Enter saves from Source', async () => {
  const user = userEvent.setup()
  const { getByRole } = render(<QuickAddWindow />)
  const source = getByRole('textbox', { name: /Source/ })
  replaceEditorDocument(getByRole('textbox', { name: 'Front' }), 'front')
  replaceEditorDocument(getByRole('textbox', { name: 'Back' }), 'back')
  fireEvent.change(source, { target: { value: '   ' } })
  act(() => source.focus())

  await user.keyboard('{Meta>}{Enter}{/Meta}')

  await waitFor(() => {
    expect(mocks.createCardContent).toHaveBeenCalledWith({
      backMd: 'back',
      frontMd: 'front',
      source: null,
      type: CardContentType.Basic,
    })
  })
})

test('creates a CLOZE card with canonical variants and a revealed search projection', async () => {
  const { getByRole } = render(<QuickAddWindow />)
  fireEvent.mouseDown(getByRole('button', { name: 'Card type: Basic' }))
  fireEvent.mouseDown(getByRole('option', { name: 'Cloze' }))
  const text = getByRole('textbox', { name: 'Text' })
  const extra = getByRole('textbox', { name: 'Extra' })
  replaceEditorDocument(
    text,
    'A {{c2::**second**::position}} and {{c1::first}} plus {{c2::two}}.',
  )
  replaceEditorDocument(extra, 'Supplemental context.')

  fireEvent.click(getByRole('button', { name: /Add/ }))

  await waitFor(() => {
    expect(mocks.createCardContent).toHaveBeenCalledWith({
      backMd: 'Supplemental context.',
      frontMd:
        'A {{c2::**second**::position}} and {{c1::first}} plus {{c2::two}}.',
      searchMd: 'A **second** and first plus two.',
      source: null,
      type: CardContentType.Cloze,
      variantKeys: ['cloze:1', 'cloze:2'],
    })
  })
  expect(mocks.dismissQuickAdd).toHaveBeenCalledTimes(1)
})

test('blocks invalid CLOZE syntax and focuses its Text editor', () => {
  const { getByRole } = render(<QuickAddWindow />)
  fireEvent.mouseDown(getByRole('button', { name: 'Card type: Basic' }))
  fireEvent.mouseDown(getByRole('option', { name: 'Cloze' }))
  const text = getByRole('textbox', { name: 'Text' })
  replaceEditorDocument(text, '`{{c1::code is literal}}`')

  fireEvent.click(getByRole('button', { name: /Add/ }))

  expect(getByRole('alert').textContent).toContain(
    'Add at least one cloze',
  )
  expect(document.activeElement).toBe(text)
  expect(mocks.createCardContent).not.toHaveBeenCalled()
})

test('the card-type dropdown is app-owned and consumes Escape before dismissal', async () => {
  const { getByRole, queryByRole } = render(<QuickAddWindow />)
  const trigger = getByRole('button', { name: 'Card type: Basic' })
  expect(queryByRole('combobox', { name: 'Card type' })).toBeNull()

  fireEvent.mouseDown(trigger)
  const listbox = getByRole('listbox', { name: 'Card type' })
  expect(listbox.classList.contains('dara-select-popover')).toBe(true)
  await waitFor(() => {
    expect(document.activeElement).toBe(
      getByRole('option', { name: 'Basic' }),
    )
  })

  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })

  expect(queryByRole('listbox', { name: 'Card type' })).toBeNull()
  expect(mocks.dismissQuickAdd).not.toHaveBeenCalled()
})

test('plain Enter in Source neither submits nor changes its value', async () => {
  const user = userEvent.setup()
  const { getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const source = getByRole('textbox', { name: /Source/ }) as HTMLInputElement
  await waitFor(() => expect(document.activeElement).toBe(front))
  fireEvent.change(source, { target: { value: 'notes' } })
  act(() => source.focus())

  await user.keyboard('{Enter}')

  expect(source.value).toBe('notes')
  expect(document.activeElement).toBe(source)
  expect(mocks.createCardContent).not.toHaveBeenCalled()
})

test('a failed save retains all values and a retry can succeed', async () => {
  mocks.createCardContent.mockRejectedValueOnce(new Error('database unavailable'))
  const { getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const back = getByRole('textbox', { name: 'Back' })
  const source = getByRole('textbox', { name: /Source/ }) as HTMLInputElement
  replaceEditorDocument(front, 'front')
  replaceEditorDocument(back, 'back')
  fireEvent.change(source, { target: { value: 'source' } })

  fireEvent.click(getByRole('button', { name: /Add/ }))
  await waitFor(() => expect(getByRole('alert').textContent).toContain('database unavailable'))

  expect(editorView(front).state.doc.textContent).toBe('front')
  expect(editorView(back).state.doc.textContent).toBe('back')
  expect(source.value).toBe('source')
  expect(mocks.dismissQuickAdd).not.toHaveBeenCalled()

  fireEvent.click(getByRole('button', { name: /Add/ }))
  await waitFor(() => expect(mocks.dismissQuickAdd).toHaveBeenCalledTimes(1))
})

test('validation focuses the missing field and composition Escape does not dismiss', async () => {
  const { getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const back = getByRole('textbox', { name: 'Back' })
  replaceEditorDocument(front, 'front')

  fireEvent.click(getByRole('button', { name: /Add/ }))
  expect(document.activeElement).toBe(back)
  expect(mocks.createCardContent).not.toHaveBeenCalled()

  fireEvent.keyDown(back, { isComposing: true, key: 'Escape' })
  expect(mocks.dismissQuickAdd).not.toHaveBeenCalled()
  fireEvent.keyDown(back, { key: 'Escape' })
  await waitFor(() => expect(mocks.dismissQuickAdd).toHaveBeenCalledTimes(1))
})

test('the app-owned code-language menu closes without dismissing Quick Add', () => {
  const { getByRole, queryByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const source = getByRole('textbox', { name: /Source/ })
  replaceEditorDocument(front, '```typescript\nconst answer = 42\n```')

  const trigger = getByRole('button', {
    name: 'Code language: TypeScript',
  })
  fireEvent.mouseDown(trigger)
  expect(getByRole('listbox', { name: 'Code language' })).not.toBeNull()

  fireEvent.pointerDown(source)
  expect(queryByRole('listbox', { name: 'Code language' })).toBeNull()
  expect(mocks.dismissQuickAdd).not.toHaveBeenCalled()

  fireEvent.mouseDown(trigger)
  fireEvent.keyDown(getByRole('option', { name: 'TypeScript' }), {
    key: 'Escape',
  })
  expect(queryByRole('listbox', { name: 'Code language' })).toBeNull()
  expect(mocks.dismissQuickAdd).not.toHaveBeenCalled()
})

test('the math dialog consumes Escape before Quick Add dismissal', () => {
  const { getAllByRole, getByRole, queryByRole } = render(<QuickAddWindow />)

  fireEvent.mouseDown(getAllByRole('button', { name: 'Inline math' })[0]!)
  const formula = getByRole('textbox', { name: 'Formula' })
  expect(getByRole('dialog', { name: 'Inline math editor' })).not.toBeNull()

  fireEvent.keyDown(formula, { key: 'Escape' })

  expect(queryByRole('dialog', { name: 'Inline math editor' })).toBeNull()
  expect(mocks.dismissQuickAdd).not.toHaveBeenCalled()
})

test('Escape from a nested code block dismisses Quick Add', async () => {
  const { container, getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  replaceEditorDocument(front, '```python\npass\n```')

  await waitFor(() => {
    expect(
      container.querySelector('.dara-code-block-editor .cm-content'),
    ).not.toBeNull()
  })
  const codeContent = container.querySelector<HTMLElement>(
    '.dara-code-block-editor .cm-content',
  )
  if (!codeContent) {
    throw new Error('CodeMirror content not found')
  }

  fireEvent.keyDown(codeContent, { key: 'Escape' })

  await waitFor(() => expect(mocks.dismissQuickAdd).toHaveBeenCalledTimes(1))
})

test('Control-Enter exits a front code block without submitting or focusing Back', async () => {
  const { container, getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const back = getByRole('textbox', { name: 'Back' })
  replaceEditorDocument(front, '```python\npass\n```')

  await waitFor(() => {
    expect(
      container.querySelector('.dara-code-block-editor .cm-content'),
    ).not.toBeNull()
  })
  const codeContent = container.querySelector<HTMLElement>(
    '.dara-code-block-editor .cm-content',
  )
  if (!codeContent) {
    throw new Error('CodeMirror content not found')
  }
  const codeView = CodeMirrorView.findFromDOM(codeContent)
  if (!codeView) {
    throw new Error('CodeMirror view not found')
  }
  act(() => {
    codeView.focus()
    codeView.dispatch({
      selection: { anchor: codeView.state.doc.length },
    })
  })

  fireEvent.keyDown(codeContent, { ctrlKey: true, key: 'Enter' })

  expect(editorView(front).state.doc.lastChild?.type.name).toBe('paragraph')
  expect(document.activeElement).toBe(front)
  expect(document.activeElement).not.toBe(back)
  expect(mocks.createCardContent).not.toHaveBeenCalled()
})

test('a nested code block is not an extra Tab stop between Front and Back', async () => {
  const user = userEvent.setup()
  const { container, getByRole } = render(<QuickAddWindow />)
  const front = getByRole('textbox', { name: 'Front' })
  const back = getByRole('textbox', { name: 'Back' })
  replaceEditorDocument(front, '```python\npass\n```')

  await waitFor(() => {
    expect(
      container.querySelector('.dara-code-block-editor .cm-content'),
    ).not.toBeNull()
  })
  const codeContent = container.querySelector<HTMLElement>(
    '.dara-code-block-editor .cm-content',
  )
  if (!codeContent) {
    throw new Error('CodeMirror content not found')
  }
  const codeView = CodeMirrorView.findFromDOM(codeContent)
  if (!codeView) {
    throw new Error('CodeMirror view not found')
  }
  expect(codeContent.tabIndex).toBe(-1)
  act(() => codeView.focus())

  await user.tab()
  expect(document.activeElement).toBe(back)
  await user.tab({ shift: true })
  expect(front.contains(document.activeElement)).toBe(true)
  expect(document.activeElement).toBe(codeContent)
})

function replaceEditorDocument(element: HTMLElement, value: string) {
  const view = editorView(element)
  const replacement = parseDaraMarkdown(value, daraEditorSchema)
  act(() => {
    view.dispatch(
      view.state.tr.replaceWith(
        0,
        view.state.doc.content.size,
        replacement.content,
      ),
    )
  })
}

function editorView(element: HTMLElement) {
  const view = richTextEditorViewFromDOM(element)
  if (!view) {
    throw new Error('EditorView not found')
  }
  return view
}
