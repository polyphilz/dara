import { describe, expect, test } from 'vitest'
import { findTypographyViolations } from '../../../scripts/check-typography-contracts.mjs'

const FEATURE_STYLESHEET = 'src/windows/main/main-window.css'

function violations(contents: string, path = FEATURE_STYLESHEET) {
  return findTypographyViolations(path, contents, path)
}

describe('rejects feature-local typography', () => {
  test('a raw pixel font size fails', () => {
    const found = violations('.setting-note {\n  font-size: 12px;\n}\n')
    expect(found).toHaveLength(1)
    expect(found[0].check).toBe('font-size')
    expect(found[0].line).toBe(2)
  })

  test('a raw rem font size fails', () => {
    expect(violations('.a {\n  font-size: 0.75rem;\n}\n')).toHaveLength(1)
  })

  test('a font shorthand with a pixel size fails', () => {
    const found = violations('.a {\n  font: 650 12px/1.1 var(--font-family-app);\n}\n')
    expect(found[0].check).toBe('font shorthand')
  })

  test('raw weights, tracking, and leading fail', () => {
    const found = violations(
      '.a {\n  font-weight: 750;\n  letter-spacing: 0.09em;\n  line-height: 1.45;\n}\n',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      'font-weight',
      'letter-spacing',
      'line-height',
    ])
  })

  test('a numeric fontSize in TypeScript fails', () => {
    const found = violations(
      'const style = { fontSize: 13 }\n',
      'src/markdown/CodeBlockNodeView.ts',
    )
    expect(found[0].check).toBe('fontSize')
  })

  test('a pixel fontSize string in TypeScript fails', () => {
    const found = violations(
      "const style = { fontSize: '13px' }\n",
      'src/markdown/CodeBlockNodeView.ts',
    )
    expect(found[0].check).toBe('fontSize')
  })
})

describe('accepts the central typography system', () => {
  test('type tokens pass', () => {
    expect(
      violations(
        '.setting-note {\n' +
          '  font-size: var(--type-size-supporting);\n' +
          '  font-weight: var(--type-weight-supporting);\n' +
          '  line-height: var(--type-leading-supporting);\n' +
          '  letter-spacing: var(--type-tracking-supporting);\n' +
          '}\n',
      ),
    ).toEqual([])
  })

  test('relative authored-content ratios pass', () => {
    expect(
      violations(
        '.dara-markdown h1 {\n  font-size: 1.45em;\n}\n',
        'src/markdown/markdown-renderer.css',
      ),
    ).toEqual([])
  })

  test('inherit keywords pass', () => {
    expect(violations('.a {\n  font: inherit;\n  font-size: inherit;\n}\n')).toEqual(
      [],
    )
  })

  test('non-text dimensions pass', () => {
    expect(
      violations('.a {\n  width: 12px;\n  gap: 1.5rem;\n  border-radius: 9px;\n}\n'),
    ).toEqual([])
  })

  test('comments and documentation pass', () => {
    expect(
      violations('/* The old value was font-size: 10px; */\n.a {\n  color: red;\n}\n'),
    ).toEqual([])
  })
})

describe('documented dynamic exceptions', () => {
  test('the activity graph label size is allowed in Home.tsx only', () => {
    const source = '<ActivityCalendar fontSize={12 * ACTIVITY_GRAPH_SCALE} />\n'
    expect(
      violations(source, 'src/windows/main/Home.tsx'),
    ).toEqual([])
    expect(
      violations(source, 'src/windows/main/Settings.tsx'),
    ).toHaveLength(1)
  })

  test('the mask number size is allowed in OcclusionEditor.tsx only', () => {
    const source = '<text fontSize={maskNumberBadgeRadius * 1.15} />\n'
    expect(violations(source, 'src/occlusion/OcclusionEditor.tsx')).toEqual([])
    expect(violations(source, 'src/occlusion/OcclusionReview.tsx')).toHaveLength(1)
  })
})
