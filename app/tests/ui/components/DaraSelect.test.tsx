import { useState } from 'react'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { DaraSelect } from '../../../src/components/DaraSelect.tsx'

const TestValue = {
  Basic: 'BASIC',
  Cloze: 'CLOZE',
} as const

type TestValue = (typeof TestValue)[keyof typeof TestValue]

const options = [
  { label: 'Basic', value: TestValue.Basic },
  { label: 'Cloze', value: TestValue.Cloze },
] as const

test('opens an app-owned listbox and supports keyboard selection', async () => {
  const onSelect = vi.fn()

  function Harness() {
    const [value, setValue] = useState<TestValue>(TestValue.Basic)
    return (
      <DaraSelect
        ariaLabel="Card type"
        onSelect={(nextValue) => {
          setValue(nextValue)
          onSelect(nextValue)
        }}
        options={options}
        value={value}
      />
    )
  }

  const { getByRole, queryByRole } = render(<Harness />)
  const trigger = getByRole('button', { name: 'Card type: Basic' })
  expect(trigger.tagName).toBe('BUTTON')

  fireEvent.keyDown(trigger, { key: 'ArrowDown' })
  const listbox = getByRole('listbox', { name: 'Card type' })
  expect(listbox.classList.contains('dara-select-popover')).toBe(true)
  await waitFor(() => {
    expect(document.activeElement).toBe(
      getByRole('option', { name: 'Basic' }),
    )
  })

  fireEvent.keyDown(listbox, { key: 'ArrowDown' })
  expect(document.activeElement).toBe(getByRole('option', { name: 'Cloze' }))
  fireEvent.keyDown(listbox, { key: 'Enter' })

  expect(onSelect).toHaveBeenCalledWith(TestValue.Cloze)
  expect(queryByRole('listbox', { name: 'Card type' })).toBeNull()
  expect(
    getByRole('button', { name: 'Card type: Cloze' }),
  ).toBeTruthy()
})

test('Escape closes the listbox and returns focus to its trigger', async () => {
  const { getByRole, queryByRole } = render(
    <DaraSelect
      ariaLabel="Card type"
      onSelect={vi.fn()}
      options={options}
      value={TestValue.Basic}
    />,
  )
  const trigger = getByRole('button', { name: 'Card type: Basic' })
  fireEvent.mouseDown(trigger)
  const listbox = getByRole('listbox', { name: 'Card type' })
  await waitFor(() => {
    expect(document.activeElement).toBe(
      getByRole('option', { name: 'Basic' }),
    )
  })

  fireEvent.keyDown(listbox, { key: 'Escape' })

  expect(queryByRole('listbox', { name: 'Card type' })).toBeNull()
  await waitFor(() => expect(document.activeElement).toBe(trigger))
})
