import { act, fireEvent, render, waitFor } from '@testing-library/react'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { useState, type ComponentProps } from 'react'
import { beforeEach, expect, test, vi } from 'vitest'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { parseDaraMarkdown } from '../../../src/markdown/markdown-conversion.ts'
import {
  CardContentReviewStatus,
  CardContentType,
  OcclusionMaskColor,
  OcclusionMode,
  ReviewCardStatus,
  SearchExecutionMode,
  SemanticSearchPhase,
  type CardContentListItem,
  type SearchCardContentResult,
  type SemanticSearchStatus,
} from '../../../src/review/contracts.ts'
import { ImageOcrStatus } from '../../../src/media/image-reference.ts'
import { ReviewCardState } from '../../../src/scheduling/index.ts'

const mocks = vi.hoisted(() => ({
  deleteCardContent: vi.fn(),
  listen: vi.fn(),
  loadCardContent: vi.fn(),
  searchCardContent: vi.fn(),
  searchStatus: vi.fn(),
  setCardContentSuspended: vi.fn(),
  updateCardContent: vi.fn(),
  renewMediaLease: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: mocks.listen,
}))

vi.mock('../../../src/media/gateway.ts', () => ({
  ingestClipboardImage: vi.fn(),
  ingestImageFile: vi.fn(),
  renewMediaLease: mocks.renewMediaLease,
}))

vi.mock('../../../src/review/index.ts', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../src/review/index.ts')>()),
  createCardContent: vi.fn(),
  deleteCardContent: mocks.deleteCardContent,
  loadCardContent: mocks.loadCardContent,
  searchCardContent: mocks.searchCardContent,
  searchStatus: mocks.searchStatus,
  setCardContentSuspended: mocks.setCardContentSuspended,
  updateCardContent: mocks.updateCardContent,
}))

import { CardBrowser as RoutedCardBrowser } from '../../../src/windows/main/CardBrowser.tsx'

function CardBrowser(
  props: Omit<
    ComponentProps<typeof RoutedCardBrowser>,
    | 'editingCardContentId'
    | 'onEdit'
    | 'onExitEdit'
    | 'onSelect'
    | 'selectedCardContentId'
  >,
) {
  const [selectedCardContentId, setSelectedCardContentId] = useState<
    string | null
  >(null)
  return (
    <RoutedCardBrowser
      {...props}
      editingCardContentId={null}
      onEdit={() => undefined}
      onExitEdit={() => undefined}
      onSelect={setSelectedCardContentId}
      selectedCardContentId={selectedCardContentId}
    />
  )
}

const activeItem: CardContentListItem = {
  cardContent: {
    id: '01980c8e-6c00-7000-8000-000000000101',
    createdAt: 1_000,
    updatedAt: 2_000,
    type: CardContentType.Basic,
    frontMd: 'Why is **copper** conductive?',
    backMd: 'It has mobile electrons.',
    source: 'EE notes',
  },
  lifecycleUpdatedAt: 3_000,
  reviewCards: [
    {
      id: '01980c8e-6c00-7000-8000-000000000102',
      status: ReviewCardStatus.Active,
      variantKey: 'basic',
      state: ReviewCardState.New,
      dueAt: null,
      dueStudyDay: null,
      lastReviewAt: null,
    },
  ],
  reviewStatus: CardContentReviewStatus.Active,
}

const readySemanticStatus: SemanticSearchStatus = {
  phase: SemanticSearchPhase.Ready,
  downloadedBytes: 232_883_776,
  modelBytes: 232_883_776,
  indexedDocuments: 3,
  totalDocuments: 3,
  message: null,
}

function searchResult(
  items: CardContentListItem[],
  mode: SearchExecutionMode = SearchExecutionMode.Browse,
  semanticStatus: SemanticSearchStatus = readySemanticStatus,
): SearchCardContentResult {
  return { items, mode, semanticStatus }
}

const clozeItem: CardContentListItem = {
  cardContent: {
    id: '01980c8e-6c00-7000-8000-000000000201',
    createdAt: 1_000,
    updatedAt: 2_000,
    type: CardContentType.Cloze,
    frontMd: 'The {{c1::capital}} of France is {{c2::Paris::city}}.',
    backMd: 'A geography prompt.',
    source: 'Geography notes',
  },
  lifecycleUpdatedAt: 3_000,
  reviewCards: [
    {
      id: '01980c8e-6c00-7000-8000-000000000202',
      status: ReviewCardStatus.Active,
      variantKey: 'cloze:1',
      state: ReviewCardState.New,
      dueAt: null,
      dueStudyDay: null,
      lastReviewAt: null,
    },
    {
      id: '01980c8e-6c00-7000-8000-000000000203',
      status: ReviewCardStatus.Active,
      variantKey: 'cloze:2',
      state: ReviewCardState.New,
      dueAt: null,
      dueStudyDay: null,
      lastReviewAt: null,
    },
  ],
  reviewStatus: CardContentReviewStatus.Active,
}

const occlusionItem: CardContentListItem = {
  cardContent: {
    id: '01980c8e-6c00-7000-8000-000000000301',
    createdAt: 1_000,
    updatedAt: 2_000,
    type: CardContentType.Occlusion,
    frontMd: '',
    backMd: '',
    source: null,
    occlusion: {
      id: '01980c8e-6c00-7000-8000-000000000302',
      sourceImage: {
        id: '01980c8e-6c00-7000-8000-000000000303',
        mimeType: 'image/webp',
        naturalWidth: 1_000,
        naturalHeight: 500,
        ocrStatus: ImageOcrStatus.Ready,
      },
      mode: OcclusionMode.HideOneGuessOne,
      layers: [
        {
          id: '01980c8e-6c00-7000-8000-000000000304',
          label: 'Output',
          masks: [
            {
              id: '01980c8e-6c00-7000-8000-000000000305',
              x: 0.1,
              y: 0.1,
              width: 0.2,
              height: 0.1,
              color: OcclusionMaskColor.White,
            },
          ],
        },
        {
          id: '01980c8e-6c00-7000-8000-000000000306',
          label: 'Input',
          masks: [
            {
              id: '01980c8e-6c00-7000-8000-000000000307',
              x: 0.7,
              y: 0.75,
              width: 0.2,
              height: 0.15,
              color: OcclusionMaskColor.Black,
            },
          ],
        },
      ],
    },
  },
  lifecycleUpdatedAt: 3_000,
  reviewCards: [
    {
      id: '01980c8e-6c00-7000-8000-000000000308',
      status: ReviewCardStatus.Active,
      variantKey: 'layer:01980c8e-6c00-7000-8000-000000000304',
      state: ReviewCardState.Review,
      dueAt: null,
      dueStudyDay: 99_999,
      lastReviewAt: 1_000,
    },
    {
      id: '01980c8e-6c00-7000-8000-000000000309',
      status: ReviewCardStatus.Active,
      variantKey: 'layer:01980c8e-6c00-7000-8000-000000000306',
      state: ReviewCardState.New,
      dueAt: null,
      dueStudyDay: null,
      lastReviewAt: null,
    },
  ],
  reviewStatus: CardContentReviewStatus.Active,
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.listen.mockResolvedValue(() => undefined)
  mocks.searchCardContent.mockResolvedValue(searchResult([activeItem]))
  mocks.searchStatus.mockResolvedValue(readySemanticStatus)
  mocks.deleteCardContent.mockResolvedValue(undefined)
  mocks.loadCardContent.mockResolvedValue(activeItem)
  mocks.setCardContentSuspended.mockResolvedValue({
    ...activeItem,
    lifecycleUpdatedAt: 3_001,
    reviewStatus: CardContentReviewStatus.Suspended,
  })
  mocks.updateCardContent.mockResolvedValue(activeItem)
  mocks.renewMediaLease.mockResolvedValue(0)
})

test('submits hybrid search on Enter and handles window-level Browse shortcuts', async () => {
  const onQueueChanged = vi.fn()
  const { getAllByText, getByLabelText, getByText } = render(
    <CardBrowser onQueueChanged={onQueueChanged} />,
  )

  await waitFor(() => {
    expect(mocks.searchCardContent).toHaveBeenCalledWith({
      query: '',
      limit: 51,
      offset: 0,
    })
  })
  expect(getByText('Why is copper conductive?')).toBeTruthy()

  const search = getByLabelText('Search cards') as HTMLInputElement
  expect(search.getAttribute('autocapitalize')).toBe('none')
  expect(search.getAttribute('autocomplete')).toBe('off')
  expect(search.getAttribute('autocorrect')).toBe('off')
  expect(search.getAttribute('spellcheck')).toBe('false')
  expect(search.getAttribute('writingsuggestions')).toBe('false')
  fireEvent.change(search, { target: { value: 'conduct' } })
  expect(mocks.searchCardContent).toHaveBeenCalledTimes(1)
  expect(getByText('Press Enter to search.')).toBeTruthy()
  fireEvent.keyDown(search, { key: 'Enter' })
  await waitFor(() => {
    expect(mocks.searchCardContent).toHaveBeenLastCalledWith({
      query: 'conduct',
      limit: 51,
      offset: 0,
    })
  })

  const outside = document.createElement('button')
  document.body.append(outside)
  outside.focus()
  fireEvent.keyDown(window, { code: 'KeyF', key: 'f', metaKey: true })
  expect(document.activeElement).toBe(search)
  expect(search.selectionStart).toBe(0)
  expect(search.selectionEnd).toBe(search.value.length)
  outside.remove()

  await waitFor(() => {
    expect(mocks.listen).toHaveBeenCalledWith('browse-command', expect.any(Function))
  })
  const nativeCommandListener = mocks.listen.mock.calls.find(
    ([eventName]) => eventName === 'browse-command',
  )?.[1] as ((event: { payload: unknown }) => void) | undefined
  act(() => {
    nativeCommandListener?.({ payload: 'TOGGLE_SELECTED_SUSPENSION' })
  })
  await waitFor(() => {
    expect(mocks.setCardContentSuspended).toHaveBeenCalledWith({
      cardContentId: activeItem.cardContent.id,
      expectedLifecycleUpdatedAt: 3_000,
      suspended: true,
    })
  })
  expect(getAllByText('Paused')).toHaveLength(2)
  expect(onQueueChanged).toHaveBeenCalledTimes(1)
})

test('does not submit an empty search on Enter', async () => {
  const { getByLabelText } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )

  await waitFor(() => expect(mocks.searchCardContent).toHaveBeenCalledTimes(1))
  fireEvent.keyDown(getByLabelText('Search cards'), { key: 'Enter' })

  expect(mocks.searchCardContent).toHaveBeenCalledTimes(1)
})

test('keeps the card list position while opening each selected card at the top', async () => {
  mocks.searchCardContent.mockResolvedValue(
    searchResult([activeItem, clozeItem]),
  )
  const { container, getAllByRole, getByText } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )
  await waitFor(() => expect(getByText('Why is copper conductive?')).toBeTruthy())
  const resultList = container.querySelector<HTMLElement>('.card-result-list')
  const detail = container.querySelector<HTMLElement>('.card-detail-content')
  if (!resultList || !detail) {
    throw new Error('Browse scroll regions were not rendered')
  }
  resultList.scrollTop = 120
  detail.scrollTop = 80

  fireEvent.click(getAllByRole('option')[1]!)

  expect(resultList.scrollTop).toBe(120)
  expect(detail.scrollTop).toBe(0)
})

test('clearing a submitted query immediately restores the all-cards view', async () => {
  const { getByLabelText, getByText } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )
  await waitFor(() => expect(getByText('Why is copper conductive?')).toBeTruthy())
  const search = getByLabelText('Search cards')

  fireEvent.change(search, { target: { value: 'copper' } })
  fireEvent.keyDown(search, { key: 'Enter' })
  await waitFor(() => {
    expect(mocks.searchCardContent).toHaveBeenLastCalledWith({
      query: 'copper',
      limit: 51,
      offset: 0,
    })
  })

  const submittedCallCount = mocks.searchCardContent.mock.calls.length
  fireEvent.change(search, { target: { value: 'coppers' } })
  fireEvent.change(search, { target: { value: 'copper' } })
  expect(mocks.searchCardContent).toHaveBeenCalledTimes(submittedCallCount)
  expect(getByText('Press Enter to search.')).toBeTruthy()

  fireEvent.change(search, { target: { value: '' } })
  await waitFor(() => {
    expect(mocks.searchCardContent).toHaveBeenLastCalledWith({
      query: '',
      limit: 51,
      offset: 0,
    })
  })
  expect(getByText('All cards')).toBeTruthy()
})

test.each([
  [
    'BASIC',
    activeItem,
    ['Front', 'Back', 'Review cards · 1', 'Source'],
  ],
  [
    'CLOZE',
    clozeItem,
    ['Text', 'Extra', 'Review cards · 2', 'Source'],
  ],
])('keeps %s authored fields before review cards', async (_, item, labels) => {
  mocks.searchCardContent.mockResolvedValue(searchResult([item]))
  const { getByRole } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )

  const article = await waitFor(() => getByRole('article'))
  expect(
    Array.from(
      article.children,
      (section) => section.firstElementChild?.textContent,
    ),
  ).toEqual(labels)
})

test('reports lexical fallback when semantic search is unavailable', async () => {
  const unavailable: SemanticSearchStatus = {
    ...readySemanticStatus,
    phase: SemanticSearchPhase.Unavailable,
    message: 'llama-server was not found',
  }
  mocks.searchCardContent.mockImplementation(({ query }: { query: string }) =>
    Promise.resolve(
      searchResult(
        [activeItem],
        query ? SearchExecutionMode.Lexical : SearchExecutionMode.Browse,
        unavailable,
      ),
    ),
  )
  const { getByLabelText, getByText } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )
  await waitFor(() =>
    expect(
      getByText('Semantic search unavailable · lexical search still works'),
    ).toBeTruthy(),
  )
  const search = getByLabelText('Search cards')
  fireEvent.change(search, { target: { value: 'copper' } })
  fireEvent.keyDown(search, { key: 'Enter' })
  await waitFor(() => expect(getByText('Lexical matches')).toBeTruthy())
})

test.each([
  SemanticSearchPhase.Verifying,
  SemanticSearchPhase.Starting,
])('shimmers while semantic search is initializing in %s', async (phase) => {
  const verifying: SemanticSearchStatus = {
    ...readySemanticStatus,
    phase,
    message: 'Initializing semantic search',
  }
  mocks.searchCardContent.mockResolvedValue(
    searchResult([activeItem], SearchExecutionMode.Browse, verifying),
  )

  const { getByRole } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )

  await waitFor(() => {
    expect(getByRole('status').classList).toContain(
      'semantic-search-status-shimmering',
    )
  })
})

test('opens the selected BASIC card for editing and tombstone deletion requires confirmation', async () => {
  const onQueueChanged = vi.fn()
  const { findByRole, getByLabelText, getByRole, getByText, queryByRole } =
    renderRoutedCardBrowser(
      { onQueueChanged },
    )
  await findByRole('searchbox', { name: 'Search cards' })
  await waitFor(() => expect(getByText('Why is copper conductive?')).toBeTruthy())

  fireEvent.keyDown(getByLabelText('Search cards'), { key: 'ArrowDown' })
  const selectedResult = getByRole('option', {
    name: /Why is copper conductive/,
  })
  expect(document.activeElement).toBe(selectedResult)
  fireEvent.keyDown(selectedResult, { key: 'Enter' })
  expect(await findByRole('heading', { name: 'Edit card' })).toBeTruthy()
  fireEvent.click(getByRole('button', { name: 'Cancel' }))

  await waitFor(() =>
    expect(queryByRole('heading', { name: 'Edit card' })).toBeNull(),
  )

  const deleteButton = getByRole('button', { name: 'Delete' })
  fireEvent.keyDown(deleteButton, { key: 'Enter' })
  expect(queryByRole('heading', { name: 'Edit card' })).toBeNull()

  fireEvent.click(deleteButton)
  expect(getByText('Delete this card? Review history will be retained.')).toBeTruthy()
  fireEvent.click(getByRole('button', { name: 'Delete card' }))
  await waitFor(() => {
    expect(mocks.deleteCardContent).toHaveBeenCalledWith({
      cardContentId: activeItem.cardContent.id,
      expectedLifecycleUpdatedAt: activeItem.lifecycleUpdatedAt,
      expectedUpdatedAt: activeItem.cardContent.updatedAt,
    })
  })
  expect(onQueueChanged).toHaveBeenCalledTimes(1)
})

test('renders and edits CLOZE content without exposing its stored delimiters', async () => {
  mocks.searchCardContent.mockResolvedValue(searchResult([clozeItem]))
  mocks.updateCardContent.mockResolvedValue({
    ...clozeItem,
    cardContent: {
      ...clozeItem.cardContent,
      updatedAt: 2_001,
      frontMd: 'The {{c1::capital}} of France is Paris in {{c3::Europe}}.',
    },
    reviewCards: [
      clozeItem.reviewCards[0]!,
      {
        ...clozeItem.reviewCards[1]!,
        id: '01980c8e-6c00-7000-8000-000000000204',
        variantKey: 'cloze:3',
      },
    ],
  })
  const onCardContentChanged = vi.fn()
  mocks.loadCardContent.mockResolvedValue(clozeItem)
  const {
    findByRole,
    getByRole,
    queryByRole,
    queryByText,
  } = renderRoutedCardBrowser({
    onCardContentChanged,
    onQueueChanged: vi.fn(),
  })

  await waitFor(() => {
    expect(
      getByRole('note', { name: 'Hidden cloze deletion' }).textContent,
    ).toBe('[...]')
  })
  expect(getByRole('article').textContent).toContain(
    'The [...] of France is Paris.',
  )
  expect(getByRole('button', { name: /Edit/ })).toBeTruthy()
  expect(queryByText(/\{\{c1::/)).toBeNull()
  expect(getByRole('article').textContent).toContain('A geography prompt.')

  fireEvent.click(getByRole('button', { name: /Cloze 2.*Due/ }))
  expect(
    getByRole('note', { name: 'Hidden cloze deletion' }).textContent,
  ).toBe('[city]')
  expect(getByRole('article').textContent).toContain(
    'The capital of France is [city].',
  )

  fireEvent.click(getByRole('button', { name: /Edit/ }))
  expect(await findByRole('heading', { name: 'Edit card' })).toBeTruthy()
  expect(queryByRole('button', { name: /Card type:/ })).toBeNull()
  replaceEditorDocument(
    getByRole('textbox', { name: 'Text' }),
    'The {{c1::capital}} of France is Paris in {{c3::Europe}}.',
  )
  fireEvent.click(getByRole('button', { name: /Save/ }))

  await waitFor(() => {
    expect(mocks.updateCardContent).toHaveBeenCalledWith(
      {
        id: clozeItem.cardContent.id,
        expectedUpdatedAt: clozeItem.cardContent.updatedAt,
        content: {
          backMd: 'A geography prompt.',
          frontMd: 'The {{c1::capital}} of France is Paris in {{c3::Europe}}.',
          searchMd: 'The capital of France is Paris in Europe.',
          source: 'Geography notes',
          type: CardContentType.Cloze,
          variantKeys: ['cloze:1', 'cloze:3'],
        },
      },
      expect.any(String),
    )
  })
  expect(onCardContentChanged).toHaveBeenCalledTimes(1)
})

test('shows every occlusion review sibling and previews the selected layer', async () => {
  mocks.searchCardContent.mockResolvedValue(searchResult([occlusionItem]))
  const { container, getByRole, getByText } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )

  await waitFor(() => expect(getByText('Image occlusion · 2 layers')).toBeTruthy())
  expect(getByText('Hide one, guess one').classList).toContain(
    'occlusion-mode-badge',
  )
  const reviewCards = getByRole('group', { name: 'Review cards' })
  expect(reviewCards.querySelectorAll('button')).toHaveLength(2)
  expect(getByRole('button', { name: /Output.*Due.*Last reviewed/ })).toBeTruthy()
  expect(getByRole('button', { name: /Input.*Due.*New.*Last reviewed.*Never/ })).toBeTruthy()
  expect(
    container.querySelector('.occlusion-review-mask')?.getAttribute('x'),
  ).toBe('100')

  fireEvent.click(getByRole('button', { name: /Input.*Due/ }))

  expect(
    container.querySelector('.occlusion-review-mask')?.getAttribute('x'),
  ).toBe('700')
})

test('loads an addressable edit and returns focus to the selected result', async () => {
  const onQueueChanged = vi.fn()
  const { findByRole, getByLabelText, getByRole, queryByRole } =
    renderRoutedCardBrowser(
      { onQueueChanged },
      activeItem.cardContent.id,
    )
  expect(await findByRole('heading', { name: 'Edit card' })).toBeTruthy()
  expect(mocks.loadCardContent).toHaveBeenCalledWith(
    activeItem.cardContent.id,
  )
  fireEvent.click(getByRole('button', { name: 'Cancel' }))

  await waitFor(() =>
    expect(queryByRole('heading', { name: 'Edit card' })).toBeNull(),
  )
  expect(getByLabelText('Search cards')).toBeTruthy()
  expect(document.activeElement).toBe(getByRole('option'))
})

test('loads Browse results incrementally beyond the first page', async () => {
  const items = Array.from({ length: 52 }, (_, index) => ({
    ...activeItem,
    cardContent: {
      ...activeItem.cardContent,
      id: `card-${index}`,
      frontMd: `Card ${index}`,
    },
    reviewCards: activeItem.reviewCards.map((card) => ({
      ...card,
      id: `review-${index}`,
    })),
  }))
  mocks.searchCardContent.mockImplementation(
    ({ offset }: { offset: number }) =>
      Promise.resolve(
        searchResult(offset === 0 ? items.slice(0, 51) : items.slice(offset)),
      ),
  )
  const { getAllByRole, getByRole } = render(
    <CardBrowser onQueueChanged={vi.fn()} />,
  )

  const loadMore = await waitFor(() =>
    getByRole('button', { name: 'Load more' }),
  )
  expect(getAllByRole('option')).toHaveLength(50)
  fireEvent.click(loadMore)
  await waitFor(() => expect(getAllByRole('option')).toHaveLength(52))
  expect(mocks.searchCardContent).toHaveBeenLastCalledWith({
    query: '',
    limit: 51,
    offset: 50,
  })
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

function renderRoutedCardBrowser(
  props: Omit<
    ComponentProps<typeof RoutedCardBrowser>,
    | 'editingCardContentId'
    | 'onEdit'
    | 'onExitEdit'
    | 'onSelect'
    | 'selectedCardContentId'
  >,
  initialEditingCardContentId: string | null = null,
) {
  function CardBrowserRouteHarness() {
    const [editingCardContentId, setEditingCardContentId] = useState(
      initialEditingCardContentId,
    )
    const [selectedCardContentId, setSelectedCardContentId] = useState<
      string | null
    >(initialEditingCardContentId)
    return (
      <RoutedCardBrowser
        {...props}
        editingCardContentId={editingCardContentId}
        onEdit={setEditingCardContentId}
        onExitEdit={() => setEditingCardContentId(null)}
        onSelect={setSelectedCardContentId}
        selectedCardContentId={selectedCardContentId}
      />
    )
  }

  const routeTree = createRootRoute({
    component: CardBrowserRouteHarness,
  })
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ['/'] }),
    routeTree,
  })
  return render(<RouterProvider router={router} />)
}
