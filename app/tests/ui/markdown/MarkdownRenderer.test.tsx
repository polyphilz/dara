import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fireEvent, render } from '@testing-library/react'
import { describe, expect, test, vi } from 'vitest'
import { MarkdownRenderer } from '../../../src/markdown/MarkdownRenderer.tsx'
import { externalHttpUrl } from '../../../src/markdown/url-policy.ts'

interface MarkdownFixture {
  id: string
  selectors: string[]
  source: string
  text: string[]
}

const fixtures = JSON.parse(
  readFileSync('tests/fixtures/markdown/dara-markdown-v1.json', 'utf8'),
) as MarkdownFixture[]

describe('Dara Markdown v1 fixture corpus', () => {
  test.each(fixtures)('$id', ({ selectors, source, text }) => {
    const { container } = render(<MarkdownRenderer source={source} />)
    const root = container.querySelector('.dara-markdown')
    assert.ok(root)
    for (const expectedText of text) {
      expect(root.textContent).toContain(expectedText)
    }
    for (const selector of selectors) {
      expect(root.querySelector(selector), `missing ${selector}`).not.toBeNull()
    }
  })
})

test('single newlines are breaks while blank lines create paragraphs', () => {
  const { container } = render(
    <MarkdownRenderer source={'line one\nline two\n\nnew paragraph'} />,
  )
  const paragraphs = container.querySelectorAll('.dara-markdown > p')
  expect(paragraphs).toHaveLength(2)
  expect(paragraphs[0]?.querySelectorAll('br')).toHaveLength(1)
})

test('known fence aliases normalize case-insensitively and unknown labels stay plain', () => {
  const source = [
    '```PY',
    'def answer():',
    '    return 42',
    '```',
    '',
    '```not-a-language',
    '<script>plain text</script>',
    '```',
  ].join('\n')
  const { container } = render(<MarkdownRenderer source={source} />)
  const python = container.querySelector('code.language-python')
  assert.ok(python)
  expect(python.classList).toContain('hljs')
  expect(python.querySelector('.hljs-keyword')).not.toBeNull()

  const unknown = container.querySelector('code.language-not-a-language')
  assert.ok(unknown)
  expect(unknown.querySelector('[class^="hljs-"]')).toBeNull()
  expect(unknown.textContent).toContain('<script>plain text</script>')
})

test('authored HTML, dangerous URLs, remote images, and trusted KaTeX commands remain inert', () => {
  const source = [
    '<script>window.evil = true</script>',
    '<style>body { display: none }</style>',
    '<iframe src="https://example.com"></iframe>',
    '<button onclick="window.evil = true">click</button>',
    '',
    '[javascript](javascript:alert(1))',
    '[file](file:///tmp/private)',
    '[custom](tauri://invoke)',
    '',
    '![remote pixels](https://example.com/tracker.png)',
    '',
    '$\\href{https://example.com}{resource command}$',
  ].join('\n')
  const { container } = render(<MarkdownRenderer source={source} />)
  const root = container.querySelector('.dara-markdown')
  assert.ok(root)

  expect(root.querySelector('script, style, iframe, button, img')).toBeNull()
  expect(root.querySelector('[onclick]')).toBeNull()
  expect(root.querySelector('a')).toBeNull()
  expect(root.textContent).toContain('<script>window.evil = true</script>')
  expect(root.textContent).toContain('remote pixels')
  expect(root.textContent).toContain('resource command')
  expect(document.body.children).toHaveLength(1)
})

test('validated HTTP links use the app-owned opener without navigating the webview', () => {
  const openExternalUrl = vi.fn<(_url: string) => Promise<void>>()
  openExternalUrl.mockResolvedValue()
  const { getByRole } = render(
    <MarkdownRenderer
      openExternalUrl={openExternalUrl}
      source="[Open docs](https://example.com/docs?q=dara)"
    />,
  )
  const link = getByRole('link', { name: 'Open docs' })
  const clickAllowed = fireEvent.click(link)

  expect(clickAllowed).toBe(false)
  expect(openExternalUrl).toHaveBeenCalledWith(
    'https://example.com/docs?q=dara',
  )
  expect(link.hasAttribute('target')).toBe(false)
})

test('task-list checkboxes are presentational only', () => {
  const { getByRole } = render(<MarkdownRenderer source="- [x] retained" />)
  expect((getByRole('checkbox') as HTMLInputElement).disabled).toBe(true)
})

test('the external URL policy accepts only absolute HTTP(S) destinations', () => {
  expect(externalHttpUrl('https://example.com/path')).toBe(
    'https://example.com/path',
  )
  expect(externalHttpUrl('http://localhost:5173')).toBe(
    'http://localhost:5173/',
  )
  expect(externalHttpUrl('/relative')).toBeNull()
  expect(externalHttpUrl('javascript:alert(1)')).toBeNull()
  expect(externalHttpUrl('file:///tmp/private')).toBeNull()
})
