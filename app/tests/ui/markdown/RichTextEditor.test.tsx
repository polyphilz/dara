import { StrictMode, createRef, useState } from 'react'
import { EditorView as CodeMirrorView } from '@codemirror/view'
import { AllSelection, NodeSelection, TextSelection } from 'prosemirror-state'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { describe, expect, test, vi } from 'vitest'
import {
  RichTextEditor,
  type RichTextEditorHandle,
} from '../../../src/markdown/RichTextEditor.tsx'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import {
  parseDaraMarkdown,
  serializeDaraMarkdown,
} from '../../../src/markdown/markdown-conversion.ts'

test('parses rich Markdown and serializes one canonical spelling', () => {
  const source = [
    '# Heading',
    '',
    '**bold** and *italic* with ~~strike~~, `code`, and $x^2$.',
    '',
    '- first',
    '- [x] done',
    '',
    '```TS',
    'const answer = 42',
    '```',
    '',
    '| A | B |',
    '| - | - |',
    '| 1 | 2 |',
  ].join('\n')

  const document = parseDaraMarkdown(source, daraEditorSchema)
  const serialized = serializeDaraMarkdown(document)

  expect(serialized).toContain('**bold** and *italic* with ~~strike~~')
  expect(serialized).toContain('$x^2$')
  expect(serialized).toContain('- [x] done')
  expect(serialized).toContain('```typescript')
  expect(serialized).toContain('| A | B |')
})

test('starts with the controlled value and reports a canonical document change', () => {
  const onChange = vi.fn()
  const { getByRole } = render(
    <RichTextEditor ariaLabel="Front" onChange={onChange} value="initial" />,
  )
  const textbox = getByRole('textbox', { name: 'Front' })
  expectWritingAssistanceDisabled(textbox)
  const view = editorView(textbox)
  expect(view.state.doc.textContent).toBe('initial')

  act(() => {
    view.dispatch(view.state.tr.insertText(' value', 8))
  })

  expect(onChange).toHaveBeenCalledTimes(1)
  expect(onChange).toHaveBeenCalledWith('initial value')
})

test('external value replacement preserves the view and does not echo onChange', () => {
  const onChange = vi.fn()
  const { getByRole, rerender } = render(
    <RichTextEditor ariaLabel="Front" onChange={onChange} value="one" />,
  )
  const firstView = editorView(getByRole('textbox', { name: 'Front' }))

  rerender(
    <RichTextEditor
      ariaLabel="Front"
      onChange={onChange}
      value="**replacement**"
    />,
  )

  const currentView = editorView(getByRole('textbox', { name: 'Front' }))
  expect(currentView).toBe(firstView)
  expect(currentView.state.doc.textContent).toBe('replacement')
  expect(currentView.state.doc.rangeHasMark(1, 12, daraEditorSchema.marks.strong!)).toBe(true)
  expect(onChange).not.toHaveBeenCalled()
})

test('imperative focus and disabled state do not recreate the editor', () => {
  const ref = createRef<RichTextEditorHandle>()
  const { getByRole, rerender } = render(
    <RichTextEditor
      ariaLabel="Front"
      onChange={() => undefined}
      ref={ref}
      value="value"
    />,
  )
  const textbox = getByRole('textbox', { name: 'Front' })
  const view = editorView(textbox)
  act(() => ref.current?.focus())
  expect(document.activeElement).toBe(textbox)

  rerender(
    <RichTextEditor
      ariaLabel="Front"
      disabled
      onChange={() => undefined}
      ref={ref}
      value="value"
    />,
  )

  expect(editorView(getByRole('textbox', { name: 'Front' }))).toBe(view)
  expect(textbox.getAttribute('contenteditable')).toBe('false')
})

test('toolbar formatting operates on the selection and preserves editor focus', () => {
  const { getByRole, view } = controlledEditor('bold')
  act(() => {
    view.dispatch(
      view.state.tr.setSelection(TextSelection.create(view.state.doc, 1, 5)),
    )
    view.focus()
  })

  fireEvent.mouseDown(getByRole('button', { name: 'Bold' }))

  expect(serializeDaraMarkdown(view.state.doc)).toBe('**bold**')
  expect(document.activeElement).toBe(getByRole('textbox', { name: 'Editor' }))
  expect(getByRole('button', { name: 'Bold' }).getAttribute('aria-pressed')).toBe('true')
})

test('keyboard shortcuts toggle bold and italic marks', () => {
  const { textbox, view } = controlledEditor('word')
  act(() => {
    view.dispatch(
      view.state.tr.setSelection(TextSelection.create(view.state.doc, 1, 5)),
    )
    view.focus()
  })

  fireEvent.keyDown(textbox, { key: 'b', metaKey: true })
  fireEvent.keyDown(textbox, { key: 'i', metaKey: true })

  const serialized = serializeDaraMarkdown(view.state.doc)
  expect(serialized).toContain('**')
  expect(serialized).toContain('*')
})

test('link editing uses an app-owned Dara input', () => {
  const { getByRole, view } = controlledEditor('word')
  act(() => {
    view.dispatch(
      view.state.tr.setSelection(TextSelection.create(view.state.doc, 1, 5)),
    )
  })

  fireEvent.mouseDown(getByRole('button', { name: 'Link' }))
  const input = getByRole('textbox', { name: 'Link URL' })
  expectWritingAssistanceDisabled(input)
  fireEvent.change(input, { target: { value: 'https://example.com' } })
  fireEvent.click(getByRole('button', { name: 'Apply' }))

  expect(serializeDaraMarkdown(view.state.doc)).toBe(
    '[word](https://example.com/)',
  )
})

test('math toolbar inserts a rendered node and serializes delimiters', () => {
  const { container, getByRole, view } = controlledEditor('formula:')
  act(() => {
    view.dispatch(view.state.tr.insertText(' ', view.state.doc.content.size - 1))
    view.dispatch(
      view.state.tr.setSelection(TextSelection.atEnd(view.state.doc)),
    )
  })

  fireEvent.mouseDown(getByRole('button', { name: 'Inline math' }))
  const formula = getByRole('textbox', { name: 'Formula' })
  expectWritingAssistanceDisabled(formula)
  fireEvent.change(formula, {
    target: { value: 'E = mc^2' },
  })
  expect(getByRole('dialog', { name: 'Inline math editor' }).querySelector('.katex')).not.toBeNull()
  fireEvent.click(getByRole('button', { name: 'Apply' }))

  expect(serializeDaraMarkdown(view.state.doc)).toBe('formula: $E = mc^2$')
  expect(container.querySelector('.dara-math-inline .katex')).not.toBeNull()
})

test('code blocks lazy-load CodeMirror and synchronize code changes', async () => {
  const { container, getByRole, view } = controlledEditor(
    '```typescript\nconst answer = 42\n```',
  )

  await waitFor(() => {
    expect(container.querySelector('.dara-code-block-editor')).not.toBeNull()
  })
  const codeEditor = container.querySelector<HTMLElement>(
    '.dara-code-block-editor .cm-content',
  )
  if (!codeEditor) {
    throw new Error('CodeMirror content not found')
  }
  expectWritingAssistanceDisabled(codeEditor)
  const codeView = CodeMirrorView.findFromDOM(codeEditor)
  if (!codeView) {
    throw new Error('CodeMirror view not found')
  }
  act(() => {
    codeView.focus()
    codeView.dispatch({
      changes: {
        from: codeView.state.doc.length,
        insert: '\nconsole.log(answer)',
      },
    })
  })

  expect(serializeDaraMarkdown(view.state.doc)).toContain(
    'console.log(answer)',
  )
  fireEvent.mouseDown(
    getByRole('button', { name: 'Code language: TypeScript' }),
  )
  fireEvent.mouseDown(getByRole('option', { name: 'Python' }))
  expect(serializeDaraMarkdown(view.state.doc)).toContain('```python')
  await waitFor(() => {
    const codeBlock = container.querySelector('.dara-code-block-editor')
    expect(
      codeBlock,
      container.querySelector('.rich-text-editor-surface')?.innerHTML,
    ).not.toBeNull()
    expect(codeBlock?.getAttribute('data-language-label')).toBe('Python')
  })
})

test('code-block Command-A progresses from code text to the whole rich document', async () => {
  const { container, textbox, view } = controlledEditor(
    'Before\n\n```text\nconst answer = 42\n```\n\nAfter',
  )

  const codeView = await embeddedCodeView(container)
  act(() => codeView.focus())
  fireEvent.keyDown(codeView.contentDOM, { key: 'a', metaKey: true })

  expect(codeView.state.selection.main.from).toBe(0)
  expect(codeView.state.selection.main.to).toBe(codeView.state.doc.length)
  expect(view.state.selection).not.toBeInstanceOf(AllSelection)

  fireEvent.keyDown(codeView.contentDOM, { key: 'a', metaKey: true })

  expect(view.state.selection).toBeInstanceOf(AllSelection)
  expect(document.activeElement).toBe(textbox)
  fireEvent.keyDown(textbox, { key: 'Backspace' })
  expect(view.state.doc.textContent).toBe('')
  expect(view.state.doc.firstChild?.type.name).toBe('paragraph')
})

test('an empty code block survives Delete but Command-A can select the outer document', async () => {
  const { container, textbox, view } = controlledEditor(
    'Before\n\n```text\n\n```',
  )

  const codeView = await embeddedCodeView(container)
  act(() => codeView.focus())
  fireEvent.keyDown(codeView.contentDOM, { key: 'Backspace' })
  expect(view.state.doc.lastChild?.type.name).toBe('code_block')

  fireEvent.keyDown(codeView.contentDOM, { key: 'a', metaKey: true })
  expect(view.state.selection).toBeInstanceOf(AllSelection)
  expect(document.activeElement).toBe(textbox)

  fireEvent.keyDown(textbox, { key: 'Backspace' })
  expect(view.state.doc.textContent).toBe('')
  expect(view.state.doc.firstChild?.type.name).toBe('paragraph')
})

test('Strict Mode creates no duplicate transaction', () => {
  const onChange = vi.fn()
  const { getByRole } = render(
    <StrictMode>
      <RichTextEditor ariaLabel="Front" onChange={onChange} value="" />
    </StrictMode>,
  )
  const view = editorView(getByRole('textbox', { name: 'Front' }))
  act(() => view.dispatch(view.state.tr.insertText('x')))
  expect(onChange).toHaveBeenCalledTimes(1)
})

test('loads a canonical image token as a resizable block node', () => {
  const imageId = '01980c8e-6c00-7000-8000-000000000201'
  const { container, textbox, view } = controlledEditor(
    `Before\n\n{{image:${imageId};width=70%}}\n\nAfter`,
  )

  const image = container.querySelector<HTMLImageElement>('.dara-editor-image img')
  expect(image?.src).toBe(`dara-media://localhost/image/${imageId}`)
  expect(image?.closest('figure')?.style.width).toBe('70%')

  const imagePosition = view.state.doc.child(0).nodeSize
  act(() => {
    view.dispatch(
      view.state.tr.setSelection(NodeSelection.create(view.state.doc, imagePosition)),
    )
  })
  fireEvent.keyDown(textbox, { altKey: true, key: 'ArrowLeft' })
  expect(serializeDaraMarkdown(view.state.doc)).toContain(
    `{{image:${imageId};width=65%}}`,
  )
})

test('image paste stays pending until durable ingestion returns an image ID', async () => {
  const imageId = '01980c8e-6c00-7000-8000-000000000202'
  type PendingRecord = {
    id: string
    mimeType: string
    naturalHeight: number
    naturalWidth: number
    ocrStatus: 'PENDING'
  }
  let resolveImage!: (record: PendingRecord) => void
  const ingestImage = vi.fn(
    () =>
      new Promise<PendingRecord>((resolve) => {
        resolveImage = resolve
      }),
  )
  const pending = vi.fn()
  function Harness() {
    const [value, setValue] = useState('Question')
    return (
      <RichTextEditor
        ariaLabel="Editor"
        ingestImage={ingestImage}
        onChange={setValue}
        onPendingMediaChange={pending}
        value={value}
      />
    )
  }
  const rendered = render(<Harness />)
  const textbox = rendered.getByRole('textbox', { name: 'Editor' })
  const view = editorView(textbox)

  fireEvent.paste(textbox, {
    clipboardData: { items: [{ type: 'image/png' }] },
  })
  expect(ingestImage).toHaveBeenCalledTimes(1)
  expect(
    rendered.getByRole('status', { name: 'Processing pasted image' }),
  ).toBeTruthy()
  expect(pending).toHaveBeenLastCalledWith(true)

  await act(async () => {
    resolveImage({
      id: imageId,
      mimeType: 'image/webp',
      naturalHeight: 600,
      naturalWidth: 800,
      ocrStatus: 'PENDING',
    })
    await Promise.resolve()
  })

  expect(rendered.queryByRole('status')).toBeNull()
  expect(rendered.container.querySelector('.dara-editor-image img')).not.toBeNull()
  expect(serializeDaraMarkdown(view.state.doc)).toContain(
    `{{image:${imageId};width=100%}}`,
  )
  expect(pending).toHaveBeenLastCalledWith(false)
})

describe('keyboard structure', () => {
  test('closing a single-backtick pair converts its contents to inline code', () => {
    const { view } = controlledEditor('')

    typeText(view, '`what what`')

    expect(view.state.doc.textContent).toBe('what what')
    expect(
      view.state.doc.rangeHasMark(
        1,
        view.state.doc.content.size - 1,
        daraEditorSchema.marks.code!,
      ),
    ).toBe(true)
    expect(serializeDaraMarkdown(view.state.doc)).toBe('`what what`')
  })

  test('Backspace after inline-code conversion restores the literal backticks', () => {
    const { textbox, view } = controlledEditor('')

    typeText(view, '`what what`')
    fireEvent.keyDown(textbox, { key: 'Backspace' })

    expect(view.state.doc.textContent).toBe('`what what`')
    expect(
      view.state.doc.rangeHasMark(
        1,
        view.state.doc.content.size - 1,
        daraEditorSchema.marks.code!,
      ),
    ).toBe(false)
  })

  test('single backticks remain literal inside a code block', () => {
    const { view } = controlledEditor('```text\nvalue\n```')

    typeText(view, '`inside`')

    expect(view.state.doc.firstChild?.type.name).toBe('code_block')
    expect(view.state.doc.textContent).toContain('`inside`')
  })

  test('three backticks convert a paragraph to a code block', () => {
    const { view } = controlledEditor('')

    typeText(view, '```')

    expect(view.state.doc.firstChild?.type.name).toBe('code_block')
    expect(view.state.doc.textContent).toBe('')
  })

  test('Backspace immediately after code-block conversion restores the backticks', () => {
    const { textbox, view } = controlledEditor('')

    typeText(view, '```')
    fireEvent.keyDown(textbox, { key: 'Backspace' })

    expect(view.state.doc.firstChild?.type.name).toBe('paragraph')
    expect(view.state.doc.textContent).toBe('```')
  })

  test.each(['-', '*', '•'])(
    '%s followed by Space starts a bulleted list',
    (marker) => {
      const { view } = controlledEditor('')

      typeText(view, `${marker} first`)

      expect(view.state.doc.firstChild?.type.name).toBe('bullet_list')
      expect(serializeDaraMarkdown(view.state.doc)).toBe('- first')
    },
  )

  test('a number and period followed by Space starts an ordered list', () => {
    const { view } = controlledEditor('')

    typeText(view, '1. first')

    expect(view.state.doc.firstChild?.type.name).toBe('ordered_list')
    expect(view.state.doc.firstChild?.attrs.order).toBe(1)
    expect(serializeDaraMarkdown(view.state.doc)).toBe('1. first')
  })

  test('Backspace immediately after list conversion restores the typed prefix', () => {
    const { textbox, view } = controlledEditor('')

    typeText(view, '- ')
    fireEvent.keyDown(textbox, { key: 'Backspace' })

    expect(view.state.doc.firstChild?.type.name).toBe('paragraph')
    expect(view.state.doc.textContent).toBe('- ')
  })

  test('Shift-Enter creates a hard break that serializes as a visible newline', () => {
    const { textbox, view } = controlledEditor('one')
    act(() => {
      view.dispatch(
        view.state.tr.setSelection(TextSelection.atEnd(view.state.doc)),
      )
      view.focus()
    })
    fireEvent.keyDown(textbox, { key: 'Enter', shiftKey: true })
    act(() => view.dispatch(view.state.tr.insertText('two')))
    expect(serializeDaraMarkdown(view.state.doc)).toBe('one\ntwo')
  })
})

function controlledEditor(initialValue: string) {
  function Harness() {
    const [value, setValue] = useState(initialValue)
    return (
      <RichTextEditor ariaLabel="Editor" onChange={setValue} value={value} />
    )
  }

  const rendered = render(<Harness />)
  const textbox = rendered.getByRole('textbox', { name: 'Editor' })
  return {
    ...rendered,
    textbox,
    view: editorView(textbox),
  }
}

function editorView(element: HTMLElement) {
  const view = richTextEditorViewFromDOM(element)
  if (!view) {
    throw new Error('EditorView not found')
  }
  return view
}

async function embeddedCodeView(container: HTMLElement) {
  await waitFor(() => {
    expect(container.querySelector('.dara-code-block-editor')).not.toBeNull()
  })
  const content = container.querySelector<HTMLElement>(
    '.dara-code-block-editor .cm-content',
  )
  if (!content) {
    throw new Error('CodeMirror content not found')
  }
  const view = CodeMirrorView.findFromDOM(content)
  if (!view) {
    throw new Error('CodeMirror view not found')
  }
  return view
}

function expectWritingAssistanceDisabled(element: HTMLElement) {
  expect(element.getAttribute('autocapitalize')).toBe('none')
  expect(element.getAttribute('autocomplete')).toBe('off')
  expect(element.getAttribute('autocorrect')).toBe('off')
  expect(element.getAttribute('spellcheck')).toBe('false')
  expect(element.getAttribute('writingsuggestions')).toBe('false')
}

function typeText(view: ReturnType<typeof editorView>, text: string) {
  for (const character of text) {
    let handled = false
    const { from, to } = view.state.selection
    const defaultTransaction = () =>
      view.state.tr.insertText(character, from, to)
    view.someProp('handleTextInput', (handler) => {
      if (handler(view, from, to, character, defaultTransaction)) {
        handled = true
        return true
      }
      return false
    })
    if (!handled) {
      act(() => view.dispatch(defaultTransaction()))
    }
  }
}
