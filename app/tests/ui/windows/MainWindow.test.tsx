import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { parseDaraMarkdown } from '../../../src/markdown/markdown-conversion.ts'
import { CardContentType } from '../../../src/review/contracts.ts'
import { ReviewControllerPhase } from '../../../src/review/controller.ts'

const mocks = vi.hoisted(() => ({
  createCardContent: vi.fn(),
  listen: vi.fn(),
  notifyCardCreated: vi.fn(),
  notifyClockChanged: vi.fn(),
  refresh: vi.fn(),
  showQuickAdd: vi.fn(),
  start: vi.fn(),
}))

const caughtUpState: {
  canUndo: boolean
  nextDueAt: number | null
  notice: string | null
  phase: typeof ReviewControllerPhase.CaughtUp
} = {
  canUndo: false,
  nextDueAt: null,
  notice: null,
  phase: ReviewControllerPhase.CaughtUp,
}

vi.mock('@tauri-apps/api/event', () => ({
  listen: mocks.listen,
}))

vi.mock('../../../src/lib/native.ts', () => ({
  native: { showQuickAdd: mocks.showQuickAdd },
}))

vi.mock('../../../src/review/index.ts', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../src/review/index.ts')>()),
  createCardContent: mocks.createCardContent,
  deleteCardContent: vi.fn(),
  searchCardContent: vi.fn().mockResolvedValue([]),
  setCardContentSuspended: vi.fn(),
  updateCardContent: vi.fn(),
  ReviewController: class {
    getSnapshot = () => caughtUpState
    notifyCardCreated = mocks.notifyCardCreated
    notifyClockChanged = mocks.notifyClockChanged
    refresh = mocks.refresh
    start = mocks.start
    subscribe = () => () => undefined
  },
  tauriReviewGateway: {},
}))

import { MainWindow } from '../../../src/windows/main/MainWindow.tsx'

beforeEach(() => {
  vi.clearAllMocks()
  caughtUpState.nextDueAt = null
  mocks.createCardContent.mockResolvedValue(undefined)
  mocks.listen.mockResolvedValue(() => undefined)
  mocks.start.mockResolvedValue(undefined)
})

afterEach(() => {
  vi.useRealTimers()
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
    expect(mocks.createCardContent).toHaveBeenCalledWith({
      backMd: 'back',
      frontMd: '**front**',
      source: 'source',
      type: CardContentType.Basic,
    })
  })
  expect(mocks.notifyCardCreated).toHaveBeenCalledTimes(1)
  expect(getByRole('heading', { name: 'Caught up for now' })).toBeTruthy()
  expect(mocks.showQuickAdd).not.toHaveBeenCalled()
})

test('automatically rechecks the queue when the next learning deadline arrives', () => {
  vi.useFakeTimers()
  const now = Date.now()
  caughtUpState.nextDueAt = now + 500
  render(<MainWindow />)

  act(() => vi.advanceTimersByTime(526))

  expect(mocks.notifyClockChanged).toHaveBeenCalledTimes(1)
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
