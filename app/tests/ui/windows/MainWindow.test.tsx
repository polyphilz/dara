import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { parseDaraMarkdown } from '../../../src/markdown/markdown-conversion.ts'
import {
  CardContentType,
  ReviewCardStatus,
  ReviewQueueLane,
  SearchExecutionMode,
  SemanticSearchPhase,
} from '../../../src/review/contracts.ts'
import {
  ReviewControllerPhase,
  type ReviewControllerState,
} from '../../../src/review/controller.ts'
import { invalidateHomeStats } from '../../../src/windows/main/home-stats-cache.ts'

const mocks = vi.hoisted(() => ({
  createCardContent: vi.fn(),
  loadHomeStats: vi.fn(),
  listen: vi.fn(),
  notifyCardCreated: vi.fn(),
  notifyClockChanged: vi.fn(),
  refresh: vi.fn(),
  reveal: vi.fn(),
  renewMediaLease: vi.fn(),
  showQuickAdd: vi.fn(),
  start: vi.fn(),
}))

const controllerStore = vi.hoisted(() => ({
  current: null as unknown,
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

vi.mock('../../../src/media/gateway.ts', () => ({
  ingestClipboardImage: vi.fn(),
  ingestImageFile: vi.fn(),
  renewMediaLease: mocks.renewMediaLease,
}))

vi.mock('../../../src/review/index.ts', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../src/review/index.ts')>()),
  createCardContent: mocks.createCardContent,
  deleteCardContent: vi.fn(),
  loadHomeStats: mocks.loadHomeStats,
  searchCardContent: vi.fn().mockResolvedValue({
    items: [],
    mode: SearchExecutionMode.Browse,
    semanticStatus: {
      phase: SemanticSearchPhase.Ready,
      downloadedBytes: 0,
      modelBytes: 0,
      indexedDocuments: 0,
      totalDocuments: 0,
      message: null,
    },
  }),
  searchStatus: vi.fn().mockResolvedValue({
    phase: SemanticSearchPhase.Ready,
    downloadedBytes: 0,
    modelBytes: 0,
    indexedDocuments: 0,
    totalDocuments: 0,
    message: null,
  }),
  setCardContentSuspended: vi.fn(),
  updateCardContent: vi.fn(),
  ReviewController: class {
    getSnapshot = () => controllerStore.current
    notifyCardCreated = mocks.notifyCardCreated
    notifyClockChanged = mocks.notifyClockChanged
    refresh = mocks.refresh
    reveal = mocks.reveal
    start = mocks.start
    subscribe = () => () => undefined
  },
  tauriReviewGateway: {},
}))

import { MainWindow } from '../../../src/windows/main/MainWindow.tsx'

beforeEach(() => {
  vi.clearAllMocks()
  invalidateHomeStats()
  caughtUpState.nextDueAt = null
  controllerStore.current = caughtUpState
  mocks.createCardContent.mockResolvedValue(undefined)
  mocks.loadHomeStats.mockResolvedValue({
    activity: [],
    reviewedToday: 7,
    queue: { new: 2, learning: 3, review: 4 },
    nextLearningDueAt: null,
  })
  mocks.listen.mockResolvedValue(() => undefined)
  mocks.renewMediaLease.mockResolvedValue(0)
  mocks.start.mockResolvedValue(undefined)
})

afterEach(() => {
  vi.useRealTimers()
})

test('Add card opens a persistent main-window editor rather than Quick Add', async () => {
  const { getByRole, queryByRole, queryByText } = render(<MainWindow />)

  fireEvent.click(getByRole('button', { name: 'Add' }))

  expect(getByRole('region', { name: 'Add a card' })).toBeTruthy()
  expect(getByRole('button', { name: 'Card type: Basic' })).toBeTruthy()
  expect(queryByRole('heading', { name: 'Add a card' })).toBeNull()
  expect(queryByText('Both fields are required.')).toBeNull()
  expect(queryByRole('heading', { name: 'Caught up for now' })).toBeNull()
  expect(mocks.showQuickAdd).not.toHaveBeenCalled()

  fireEvent.click(getByRole('button', { name: 'Cancel' }))
  await waitFor(() =>
    expect(getByRole('heading', { name: 'Review activity' })).toBeTruthy(),
  )
})

test('Escape is inert in the persistent Add view and preserves the draft', () => {
  const { getByRole } = render(<MainWindow />)
  fireEvent.click(getByRole('button', { name: 'Add' }))
  const front = getByRole('textbox', { name: 'Front' })
  replaceEditorDocument(front, 'unfinished question')

  fireEvent.keyDown(front, { key: 'Escape' })

  expect(getByRole('region', { name: 'Add a card' })).toBeTruthy()
  expect(richTextEditorViewFromDOM(front)?.state.doc.textContent).toBe(
    'unfinished question',
  )
})

test('saving in the main editor creates the card and returns home', async () => {
  const { getByRole } = render(<MainWindow />)
  fireEvent.click(getByRole('button', { name: 'Add' }))

  replaceEditorDocument(getByRole('textbox', { name: 'Front' }), '**front**')
  replaceEditorDocument(getByRole('textbox', { name: 'Back' }), 'back')
  fireEvent.change(getByRole('textbox', { name: /Source/ }), {
    target: { value: '  source  ' },
  })
  fireEvent.click(getByRole('button', { name: /Add ⌘↵/ }))

  await waitFor(() => {
    expect(mocks.createCardContent).toHaveBeenCalledWith(
      {
        backMd: 'back',
        frontMd: '**front**',
        source: 'source',
        type: CardContentType.Basic,
      },
      expect.any(String),
    )
  })
  expect(mocks.notifyCardCreated).toHaveBeenCalledTimes(1)
  expect(getByRole('heading', { name: 'Review activity' })).toBeTruthy()
  expect(mocks.showQuickAdd).not.toHaveBeenCalled()
})

test('home shows review and queue stats and opens the review flow', async () => {
  const { findByText, getByRole, queryByText } = render(<MainWindow />)

  expect(queryByText('dara')).toBeNull()
  expect(queryByText('Last 365 days')).toBeNull()
  expect(await findByText('7')).toBeTruthy()
  expect(getByRole('button', { name: /Review.*7.*reviewed today.*2.*New.*3.*Learning.*4.*Review/ })).toBeTruthy()

  fireEvent.click(getByRole('button', { name: /Review.*reviewed today/ }))

  expect(getByRole('heading', { name: 'Caught up for now' })).toBeTruthy()
  expect(mocks.refresh).toHaveBeenCalled()
})

test('renders the selected CLOZE variant in the review question', async () => {
  controllerStore.current = {
    canUndo: false,
    notice: null,
    phase: ReviewControllerPhase.Question,
    card: {
      context: {
        cardContent: {
          id: '01980c8e-6c00-7000-8000-000000000201',
          createdAt: 1_000,
          updatedAt: 2_000,
          type: CardContentType.Cloze,
          frontMd:
            'The {{c1::capital}} of France is {{c2::Paris::city}}.',
          backMd: '',
          source: null,
        },
        reviewCard: {
          id: '01980c8e-6c00-7000-8000-000000000202',
          status: ReviewCardStatus.Active,
          variantKey: 'cloze:2',
          updatedAt: 2_000,
        },
      },
      lane: ReviewQueueLane.New,
      nextNormalLaneCursor: 1,
      selectionCursor: 0,
    },
  } as unknown as ReviewControllerState
  const { findByText, getByRole, queryByText } = render(<MainWindow />)
  await findByText('7')

  fireEvent.click(getByRole('button', { name: /Review.*reviewed today/ }))

  expect(getByRole('article').textContent).toContain(
    'The capital of France is [city].',
  )
  expect(queryByText(/Paris/)).toBeNull()
  fireEvent.click(getByRole('button', { name: 'Reveal answer' }))
  expect(mocks.reveal).toHaveBeenCalledTimes(1)
})

test('uses the same Home, Add, and Browse navigation on every surface', async () => {
  const { findByText, getByRole } = render(<MainWindow />)
  await findByText('7')

  const navigation = getByRole('navigation', { name: 'Main navigation' })
  const navigationButtons = () =>
    Array.from(navigation.querySelectorAll('button'), (button) =>
      button.textContent?.trim(),
    )
  expect(navigationButtons()).toEqual(['Home', 'Add', 'Browse'])

  fireEvent.click(getByRole('button', { name: /Review.*reviewed today/ }))
  expect(getByRole('navigation', { name: 'Main navigation' })).toBe(navigation)

  fireEvent.click(getByRole('button', { name: 'Browse' }))
  expect(getByRole('navigation', { name: 'Main navigation' })).toBe(navigation)

  fireEvent.click(getByRole('button', { name: 'Add' }))
  expect(getByRole('navigation', { name: 'Main navigation' })).toBe(navigation)
  expect(navigationButtons()).toEqual(['Home', 'Add', 'Browse'])
})

test('keeps the rendered home dashboard mounted while reviewing', async () => {
  const { findByRole, getByRole } = render(<MainWindow />)
  const activityHeading = await findByRole('heading', {
    name: 'Review activity',
  })

  fireEvent.click(getByRole('button', { name: /Review.*reviewed today/ }))
  fireEvent.click(getByRole('button', { name: 'Home' }))

  expect(getByRole('heading', { name: 'Review activity' })).toBe(
    activityHeading,
  )
  expect(mocks.loadHomeStats).toHaveBeenCalledTimes(1)
})

test('automatically rechecks the queue when the next learning deadline arrives', () => {
  vi.useFakeTimers()
  const now = Date.now()
  caughtUpState.nextDueAt = now + 500
  render(<MainWindow />)

  act(() => vi.advanceTimersByTime(526))

  expect(mocks.notifyClockChanged).toHaveBeenCalledTimes(1)
})

test('reuses cached home stats when the app regains focus', async () => {
  const { findByText } = render(<MainWindow />)

  expect(await findByText('7')).toBeTruthy()
  expect(mocks.loadHomeStats).toHaveBeenCalledTimes(1)

  fireEvent.focus(window)

  await waitFor(() => {
    expect(mocks.notifyClockChanged).toHaveBeenCalledTimes(1)
  })
  expect(mocks.loadHomeStats).toHaveBeenCalledTimes(1)
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
