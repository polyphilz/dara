import { render } from '@testing-library/react'
import { expect, test } from 'vitest'
import { CardSource } from '../../../src/markdown/CardSource.tsx'

test('source metadata displays as escaped plain text, never Markdown', () => {
  const { container, getByText } = render(
    <CardSource value={'**not bold** <script>inert</script> $not math$'} />,
  )
  expect(
    getByText('Source: **not bold** <script>inert</script> $not math$'),
  ).not.toBeNull()
  expect(container.querySelector('strong, script, .katex')).toBeNull()
})
