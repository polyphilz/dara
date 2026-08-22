import { useState } from 'react'
import { fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
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

const LanguageValue = {
  Java: 'java',
  JavaScript: 'javascript',
  Python: 'python',
} as const

type LanguageValue = (typeof LanguageValue)[keyof typeof LanguageValue]

const languageOptions = [
  { label: 'Java', value: LanguageValue.Java },
  { label: 'JavaScript', value: LanguageValue.JavaScript },
  { label: 'Python', value: LanguageValue.Python },
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
  expect(
    Array.from(listbox.children).map((child) => child.getAttribute('role')),
  ).toEqual(['option', 'option'])
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

test('one ArrowDown keeps focus on the first filtered option', async () => {
  const { getByRole } = render(
    <DaraSelect<LanguageValue>
      ariaLabel="Code language"
      onSelect={vi.fn()}
      options={languageOptions}
      searchable
      value={LanguageValue.Python}
    />,
  )

  fireEvent.click(
    getByRole('button', { name: 'Code language: Python' }),
  )
  const search = getByRole('textbox', { name: 'Search code language' })
  await waitFor(() => expect(document.activeElement).toBe(search))

  fireEvent.change(search, { target: { value: 'jav' } })
  fireEvent.keyDown(search, { key: 'ArrowDown' })

  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  expect(document.activeElement).toBe(
    getByRole('option', { name: 'Java' }),
  )
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
  fireEvent.click(trigger)
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

test('a complete pointer click toggles the listbox only once after a held press', async () => {
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
  await new Promise((resolve) => window.setTimeout(resolve, 10))
  expect(queryByRole('listbox', { name: 'Card type' })).toBeNull()
  fireEvent.mouseUp(trigger)
  fireEvent.click(trigger)
  expect(getByRole('listbox', { name: 'Card type' })).toBeTruthy()

  fireEvent.mouseDown(trigger)
  await new Promise((resolve) => window.setTimeout(resolve, 10))
  fireEvent.mouseUp(trigger)
  fireEvent.click(trigger)
  expect(queryByRole('listbox', { name: 'Card type' })).toBeNull()
})

test('returns focus to a caller-owned surface after selection when requested', async () => {
  const returnTarget = document.createElement('button')
  document.body.append(returnTarget)
  const { getByRole } = render(
    <DaraSelect
      ariaLabel="Card type"
      onReturnFocus={() => returnTarget.focus()}
      onSelect={vi.fn()}
      options={options}
      value={TestValue.Basic}
    />,
  )
  fireEvent.click(getByRole('button', { name: 'Card type: Basic' }))
  fireEvent.click(getByRole('option', { name: 'Cloze' }))

  await waitFor(() => expect(document.activeElement).toBe(returnTarget))
  returnTarget.remove()
})

test('associates an external label with the select trigger', async () => {
  const user = userEvent.setup()
  const { getByLabelText, getByRole, getByText } = render(
    <>
      <label htmlFor="card-type">Card type</label>
      <DaraSelect
        ariaLabel="Card type"
        id="card-type"
        onSelect={vi.fn()}
        options={options}
        value={TestValue.Basic}
      />
    </>,
  )

  const trigger = getByLabelText('Card type')
  expect(trigger.tagName).toBe('BUTTON')
  expect(trigger.id).toBe('card-type')

  await user.click(getByText('Card type'))

  expect(getByRole('listbox', { name: 'Card type' })).toBeTruthy()
})
