import { act, fireEvent, render, waitFor } from '@testing-library/react'
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
  type CardContentListItem,
} from '../../../src/review/contracts.ts'
import { ImageOcrStatus } from '../../../src/media/image-reference.ts'
import { ReviewCardState } from '../../../src/scheduling/index.ts'

const mocks = vi.hoisted(() => ({
  deleteCardContent: vi.fn(),
  searchCardContent: vi.fn(),
  setCardContentSuspended: vi.fn(),
  updateCardContent: vi.fn(),
}))

vi.mock('../../../src/review/index.ts', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../src/review/index.ts')>()),
  createCardContent: vi.fn(),
  deleteCardContent: mocks.deleteCardContent,
  searchCardContent: mocks.searchCardContent,
  setCardContentSuspended: mocks.setCardContentSuspended,
  updateCardContent: mocks.updateCardContent,
}))

import { CardBrowser } from '../../../src/windows/main/CardBrowser.tsx'

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
  mocks.searchCardContent.mockResolvedValue([activeItem])
  mocks.deleteCardContent.mockResolvedValue(undefined)
  mocks.setCardContentSuspended.mockResolvedValue({
    ...activeItem,
    lifecycleUpdatedAt: 3_001,
    reviewStatus: CardContentReviewStatus.Suspended,
  })
  mocks.updateCardContent.mockResolvedValue(activeItem)
})

test('searches immediately and toggles the selected authored item with Command-J', async () => {
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

  const search = getByLabelText('Search cards')
  expect(search.getAttribute('autocapitalize')).toBe('none')
  expect(search.getAttribute('autocomplete')).toBe('off')
  expect(search.getAttribute('autocorrect')).toBe('off')
  expect(search.getAttribute('spellcheck')).toBe('false')
  expect(search.getAttribute('writingsuggestions')).toBe('false')
  fireEvent.change(search, { target: { value: 'conduct' } })
  await waitFor(() => {
    expect(mocks.searchCardContent).toHaveBeenLastCalledWith({
      query: 'conduct',
      limit: 51,
      offset: 0,
    })
  })

  fireEvent.keyDown(search, { key: 'j', metaKey: true })
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

test('opens the selected BASIC card for editing and tombstone deletion requires confirmation', async () => {
  const onQueueChanged = vi.fn()
  const { getByLabelText, getByRole, getByText, queryByRole } = render(
    <CardBrowser onQueueChanged={onQueueChanged} />,
  )
  await waitFor(() => expect(getByText('Why is copper conductive?')).toBeTruthy())

  fireEvent.keyDown(getByLabelText('Search cards'), { key: 'Enter' })
  expect(getByRole('heading', { name: 'Edit card' })).toBeTruthy()
  fireEvent.click(getByRole('button', { name: 'Cancel' }))

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
  mocks.searchCardContent.mockResolvedValue([clozeItem])
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
  const { getAllByText, getByRole, queryByRole, queryByText } = render(
    <CardBrowser
      onCardContentChanged={onCardContentChanged}
      onQueueChanged={vi.fn()}
    />,
  )

  await waitFor(() => {
    expect(getAllByText('The [...] of France is Paris.').length).toBeGreaterThan(0)
  })
  expect(getByRole('button', { name: /Edit/ })).toBeTruthy()
  expect(queryByText(/\{\{c1::/)).toBeNull()
  expect(getByRole('article').textContent).toContain('A geography prompt.')

  fireEvent.click(getByRole('button', { name: /Cloze 2.*Due/ }))
  expect(getAllByText('The capital of France is [city].').length).toBeGreaterThan(0)

  fireEvent.click(getByRole('button', { name: /Edit/ }))
  expect(getByRole('heading', { name: 'Edit card' })).toBeTruthy()
  expect(queryByRole('button', { name: /Card type:/ })).toBeNull()
  replaceEditorDocument(
    getByRole('textbox', { name: 'Text' }),
    'The {{c1::capital}} of France is Paris in {{c3::Europe}}.',
  )
  fireEvent.click(getByRole('button', { name: /Save/ }))

  await waitFor(() => {
    expect(mocks.updateCardContent).toHaveBeenCalledWith({
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
    })
  })
  expect(onCardContentChanged).toHaveBeenCalledTimes(1)
})

test('shows every occlusion review sibling and previews the selected layer', async () => {
  mocks.searchCardContent.mockResolvedValue([occlusionItem])
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

test('returns from edit mode when Browse is invoked again', async () => {
  const onQueueChanged = vi.fn()
  const { getByLabelText, getByRole, queryByRole, rerender } = render(
    <CardBrowser navigationToken={0} onQueueChanged={onQueueChanged} />,
  )
  await waitFor(() => expect(getByLabelText('Search cards')).toBeTruthy())
  fireEvent.keyDown(getByLabelText('Search cards'), { key: 'Enter' })
  expect(getByRole('heading', { name: 'Edit card' })).toBeTruthy()

  rerender(
    <CardBrowser navigationToken={1} onQueueChanged={onQueueChanged} />,
  )

  expect(queryByRole('heading', { name: 'Edit card' })).toBeNull()
  expect(getByLabelText('Search cards')).toBeTruthy()
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
      Promise.resolve(offset === 0 ? items.slice(0, 51) : items.slice(offset)),
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
