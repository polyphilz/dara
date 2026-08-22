import { createRef } from 'react'
import { act, fireEvent, render } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import {
  DaraFilePicker,
  type DaraFilePickerHandle,
} from '../../../src/components/DaraFilePicker.tsx'

test('opens, selects repeatedly, and closes through native dialog events', () => {
  const ref = createRef<DaraFilePickerHandle>()
  const onFile = vi.fn()
  const onFileDialogOpenChange = vi.fn()
  const { container } = render(
    <DaraFilePicker
      accept="image/*"
      onFile={onFile}
      onFileDialogOpenChange={onFileDialogOpenChange}
      ref={ref}
    />,
  )
  const input = container.querySelector<HTMLInputElement>('input[type="file"]')!
  const file = new File(['png'], 'diagram.png', { type: 'image/png' })

  act(() => ref.current?.open())
  expect(onFileDialogOpenChange).toHaveBeenLastCalledWith(true)
  fireEvent.change(input, { target: { files: [file] } })
  expect(onFile).toHaveBeenCalledWith(file)
  expect(onFileDialogOpenChange).toHaveBeenLastCalledWith(false)

  act(() => ref.current?.open())
  fireEvent.change(input, { target: { files: [file] } })
  expect(onFile).toHaveBeenCalledTimes(2)

  act(() => ref.current?.open())
  fireEvent(input, new Event('cancel'))
  expect(onFileDialogOpenChange).toHaveBeenLastCalledWith(false)

  act(() => ref.current?.open())
  fireEvent(window, new Event('focus'))
  expect(onFileDialogOpenChange).toHaveBeenLastCalledWith(false)
})

test('does not open while disabled', () => {
  const ref = createRef<DaraFilePickerHandle>()
  const onFileDialogOpenChange = vi.fn()
  render(
    <DaraFilePicker
      disabled
      onFile={vi.fn()}
      onFileDialogOpenChange={onFileDialogOpenChange}
      ref={ref}
    />,
  )

  act(() => ref.current?.open())
  expect(onFileDialogOpenChange).not.toHaveBeenCalled()
})
