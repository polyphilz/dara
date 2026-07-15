import { render } from '@testing-library/react'
import { expect, test } from 'vitest'
import { DaraInput } from '../../../src/components/DaraInput.tsx'

test('disables writing assistance on every native Dara input', () => {
  const { getByRole } = render(<DaraInput aria-label="Example" />)
  const input = getByRole('textbox', { name: 'Example' })

  expect(input.getAttribute('autocapitalize')).toBe('none')
  expect(input.getAttribute('autocomplete')).toBe('off')
  expect(input.getAttribute('autocorrect')).toBe('off')
  expect(input.getAttribute('spellcheck')).toBe('false')
  expect(input.getAttribute('writingsuggestions')).toBe('false')
})
