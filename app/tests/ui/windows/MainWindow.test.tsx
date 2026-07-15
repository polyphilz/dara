import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { parseDaraMarkdown } from '../../../src/markdown/markdown-conversion.ts'

const mocks = vi.hoisted(() => ({
  createBasicCard: vi.fn(),
  listen: vi.fn(),
  notifyCardCreated: vi.fn(),
  showQuickAdd: vi.fn(),
  start: vi.fn(),
}))

const caughtUpState = {
  canUndo: false,
  nextDueAt: null,
  notice: null,
  phase: 'CAUGHT_UP' as const,
}

vi.mock('@tauri-apps/api/event', () => ({
  listen: mocks.listen,
}))

vi.mock('../../../src/lib/native.ts', () => ({
  native: { showQuickAdd: mocks.showQuickAdd },
}))

vi.mock('../../../src/review/index.ts', () => ({
  createBasicCard: mocks.createBasicCard,
  ReviewController: class {
    getSnapshot = () => caughtUpState
    notifyCardCreated = mocks.notifyCardCreated
    start = mocks.start
    subscribe = () => () => undefined
  },
  tauriReviewGateway: {},
}))

import { MainWindow } from '../../../src/windows/main/MainWindow.tsx'

beforeEach(() => {
  vi.clearAllMocks()
  mocks.createBasicCard.mockResolvedValue(undefined)
  mocks.listen.mockResolvedValue(() => undefined)
  mocks.start.mockResolvedValue(undefined)
})

test('Add card opens a persistent main-window editor rather than Quick Add', () => {
  const { getByRole, queryByRole } = render(<MainWindow />)

  fireEvent.click(getByRole('button', { name: 'Add card' }))

  expect(getByRole('heading', { name: 'Add a card' })).toBeTruthy()
  expect(queryByRole('heading', { name: 'Caught up for now' })).toBeNull()
  expect(mocks.showQuickAdd).not.toHaveBeenCalled()

  fireEvent.click(getByRole('button', { name: 'Cancel' }))
  expect(getByRole('heading', { name: 'Caught up for now' })).toBeTruthy()
})

test('saving in the main editor creates the card and returns to review', async () => {
  const { getByRole } = render(<MainWindow />)
  fireEvent.click(getByRole('button', { name: 'Add card' }))

  replaceEditorDocument(getByRole('textbox', { name: 'Front' }), '**front**')
  replaceEditorDocument(getByRole('textbox', { name: 'Back' }), 'back')
  fireEvent.change(getByRole('textbox', { name: /Source/ }), {
    target: { value: '  source  ' },
  })
  fireEvent.click(getByRole('button', { name: /^Add/ }))

  await waitFor(() => {
    expect(mocks.createBasicCard).toHaveBeenCalledWith({
      backMd: 'back',
      frontMd: '**front**',
      source: 'source',
    })
  })
  expect(mocks.notifyCardCreated).toHaveBeenCalledTimes(1)
  expect(getByRole('heading', { name: 'Caught up for now' })).toBeTruthy()
  expect(mocks.showQuickAdd).not.toHaveBeenCalled()
})

function replaceEditorDocument(element: HTMLElement, value: string) {
  const view = richTextEditorViewFromDOM(element)
  if (!view) {
    throw new Error('EditorView not found')
  }
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
