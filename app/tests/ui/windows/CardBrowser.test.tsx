import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import { daraEditorSchema } from '../../../src/markdown/editor-schema.ts'
import { richTextEditorViewFromDOM } from '../../../src/markdown/editor-view-registry.ts'
import { parseDaraMarkdown } from '../../../src/markdown/markdown-conversion.ts'
import {
  CardContentReviewStatus,
  CardContentType,
  type CardContentListItem,
} from '../../../src/review/contracts.ts'

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
    expect(mocks.searchCardContent).toHaveBeenCalledWith({ query: '', limit: 75 })
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
      limit: 75,
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
  })
  const onCardContentChanged = vi.fn()
  const { getAllByText, getByRole, queryByRole, queryByText } = render(
    <CardBrowser
      onCardContentChanged={onCardContentChanged}
      onQueueChanged={vi.fn()}
    />,
  )

  await waitFor(() => {
    expect(getAllByText('The capital of France is Paris.').length).toBeGreaterThan(0)
  })
  expect(getByRole('button', { name: /Edit/ })).toBeTruthy()
  expect(queryByText(/\{\{c1::/)).toBeNull()
  expect(getByRole('article').textContent).toContain('A geography prompt.')

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
