import { createRef } from 'react'
import { fireEvent, render } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { DaraButton } from '../../../src/components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../../src/components/dara-button-types.ts'

test('uses the canonical surface action by default and forwards native behavior', () => {
  const onClick = vi.fn()
  const ref = createRef<HTMLButtonElement>()
  const { getByRole } = render(
    <DaraButton aria-label="Save card" onClick={onClick} ref={ref}>
      Save
    </DaraButton>,
  )

  const button = getByRole('button', { name: 'Save card' })
  expect(button.getAttribute('type')).toBe('button')
  expect(button.className).toContain('dara-button-standard')
  expect(button.className).toContain('dara-button-surface')
  expect(ref.current).toBe(button)

  fireEvent.click(button)
  expect(onClick).toHaveBeenCalledTimes(1)
})

test('supports specialized size and visual variants without losing custom classes', () => {
  const { getByRole } = render(
    <DaraButton
      className="feature-control"
      size={DaraButtonSize.Icon}
      type="submit"
      variant={DaraButtonVariant.Danger}
    >
      Delete
    </DaraButton>,
  )

  const button = getByRole('button', { name: 'Delete' })
  expect(button.getAttribute('type')).toBe('submit')
  expect(button.className).toContain('dara-button-icon')
  expect(button.className).toContain('dara-button-danger')
  expect(button.className).toContain('feature-control')
})
