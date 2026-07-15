import { fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import type { CardContentListItem } from '../../../src/review/contracts.ts'

const mocks = vi.hoisted(() => ({
  deleteCardContent: vi.fn(),
  searchCardContent: vi.fn(),
  setCardContentSuspended: vi.fn(),
  updateCardContent: vi.fn(),
}))

vi.mock('../../../src/review/index.ts', () => ({
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
    type: 'BASIC',
    frontMd: 'Why is **copper** conductive?',
    backMd: 'It has mobile electrons.',
    source: 'EE notes',
  },
  lifecycleUpdatedAt: 3_000,
  reviewStatus: 'ACTIVE',
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.searchCardContent.mockResolvedValue([activeItem])
  mocks.deleteCardContent.mockResolvedValue(undefined)
  mocks.setCardContentSuspended.mockResolvedValue({
    ...activeItem,
    lifecycleUpdatedAt: 3_001,
    reviewStatus: 'SUSPENDED',
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
