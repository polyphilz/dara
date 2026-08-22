import { describe, expect, test } from 'vitest'
import {
  findTypographyViolations,
  TypographyCheckKind,
} from '../../../scripts/check-typography-contracts.mjs'

const FEATURE_STYLESHEET = 'src/windows/main/main-window.css'

function violations(contents: string, path = FEATURE_STYLESHEET) {
  return findTypographyViolations(path, contents, path)
}

describe('rejects feature-local typography', () => {
  test('a raw pixel font size fails', () => {
    const found = violations('.setting-note {\n  font-size: 12px;\n}\n')
    expect(found).toHaveLength(1)
    expect(found[0].check).toBe(TypographyCheckKind.FontSize)
    expect(found[0].line).toBe(2)
  })

  test('a raw rem font size fails', () => {
    expect(violations('.a {\n  font-size: 0.75rem;\n}\n')).toHaveLength(1)
  })

  test('relative font sizes fail outside approved authored content', () => {
    const found = violations(
      '.a {\n  font-size: 1.45em;\n}\n' +
        '.b {\n  font-size: 120%;\n}\n' +
        '.c {\n  font-size: 11pt;\n}\n',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.FontSize,
      TypographyCheckKind.FontSize,
      TypographyCheckKind.FontSize,
    ])
  })

  test('a font shorthand with a pixel size fails', () => {
    const found = violations('.a {\n  font: 650 12px/1.1 var(--font-family-app);\n}\n')
    expect(found[0].check).toBe(TypographyCheckKind.FontShorthand)
  })

  test('raw weights, tracking, and leading fail', () => {
    const found = violations(
      '.a {\n  font-weight: 750;\n  letter-spacing: 0.09em;\n  line-height: 1.45;\n}\n',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.FontWeight,
      TypographyCheckKind.LetterSpacing,
      TypographyCheckKind.LineHeight,
    ])
  })

  test('wrapped CSS typography declarations fail', () => {
    const found = violations(
      '.a {\n' +
        '  font-size:\n    12px;\n' +
        '  font:\n    650 12px/1.1 var(--font-family-app);\n' +
        '  font-weight:\n    750;\n' +
        '  letter-spacing:\n    0.09em;\n' +
        '  line-height:\n    1.45;\n' +
        '}\n',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.FontSize,
      TypographyCheckKind.FontShorthand,
      TypographyCheckKind.FontWeight,
      TypographyCheckKind.LetterSpacing,
      TypographyCheckKind.LineHeight,
    ])
    expect(found.map((entry) => entry.line)).toEqual([2, 4, 6, 8, 10])
  })

  test('a wrapped relative font size fails', () => {
    const found = violations('.a {\n  font-size:\n    120%;\n}\n')
    expect(found).toHaveLength(1)
    expect(found[0].check).toBe(TypographyCheckKind.FontSize)
    expect(found[0].line).toBe(2)
  })

  test('a numeric fontSize in TypeScript fails', () => {
    const found = violations(
      'const style = { fontSize: 13 }\n',
      'src/markdown/CodeBlockNodeView.ts',
    )
    expect(found[0].check).toBe(TypographyCheckKind.InlineFontSize)
  })

  test('a pixel fontSize string in TypeScript fails', () => {
    const found = violations(
      "const style = { fontSize: '13px' }\n",
      'src/markdown/CodeBlockNodeView.ts',
    )
    expect(found[0].check).toBe(TypographyCheckKind.InlineFontSize)
  })

  test('computed numeric fontSize expressions fail', () => {
    const found = violations(
      'const a = { fontSize: compact ? 12 : 14 }\n' +
        'const b = { fontSize: fallback ?? 12 }\n',
      'src/markdown/CodeBlockNodeView.ts',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.InlineFontSize,
      TypographyCheckKind.InlineFontSize,
    ])
  })

  test('wrapped TypeScript and JSX fontSize assignments fail', () => {
    const found = violations(
      'const style = {\n' +
        '  fontSize:\n    compact ? 12 : 14,\n' +
        '}\n' +
        'const node = <text fontSize={\n  fallback ?? 12\n} />\n',
      'src/markdown/CodeBlockNodeView.tsx',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.InlineFontSize,
      TypographyCheckKind.InlineFontSize,
    ])
    expect(found.map((entry) => entry.line)).toEqual([2, 5])
  })

  test('camelCase weight, tracking, and leading assignments fail', () => {
    const found = violations(
      'const style = { fontWeight: 700, letterSpacing: 0.04, lineHeight: 1.2 }\n' +
        '<div fontWeight={700} letterSpacing={0.04} lineHeight={1.2} />\n',
      'src/windows/main/Settings.tsx',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.FontWeight,
      TypographyCheckKind.LetterSpacing,
      TypographyCheckKind.LineHeight,
      TypographyCheckKind.FontWeight,
      TypographyCheckKind.LetterSpacing,
      TypographyCheckKind.LineHeight,
    ])
  })

  test('wrapped camelCase weight, tracking, and leading assignments fail', () => {
    const found = violations(
      'const style = {\n' +
        '  fontWeight:\n    compact ? 600 : 700,\n' +
        '  letterSpacing:\n    dense ? 0.02 : 0.04,\n' +
        '  lineHeight:\n    fallback ?? 1.2,\n' +
        '}\n' +
        'const node = <text letterSpacing={\n  dense ? 0.02 : 0.04\n} />\n',
      'src/windows/main/Settings.tsx',
    )
    expect(found.map((entry) => entry.check)).toEqual([
      TypographyCheckKind.FontWeight,
      TypographyCheckKind.LetterSpacing,
      TypographyCheckKind.LineHeight,
      TypographyCheckKind.LetterSpacing,
    ])
    expect(found.map((entry) => entry.line)).toEqual([2, 4, 6, 9])
  })

  test('a feature-local variable cannot hide a raw type value', () => {
    const found = violations(
      ':root {\n' +
        '  --card-font-size: 12px;\n' +
        '}\n' +
        '.card {\n' +
        '  font-size: var(--card-font-size);\n' +
        '}\n',
    )
    expect(found).toHaveLength(1)
    expect(found[0].check).toBe(TypographyCheckKind.TypographyVariable)
    expect(found[0].line).toBe(2)
  })

  test('a chained feature-local variable cannot hide a raw type value', () => {
    const found = violations(
      ':root {\n' +
        '  --card-font-size: var(--card-font-size-base);\n' +
        '  --card-font-size-base: 0.75rem;\n' +
        '}\n' +
        '.card {\n' +
        '  font-size: var(--card-font-size);\n' +
        '}\n',
    )
    expect(found).toHaveLength(1)
    expect(found[0].check).toBe(TypographyCheckKind.TypographyVariable)
    expect(found[0].line).toBe(3)
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

  test('wrapped type tokens pass', () => {
    expect(
      violations(
        '.setting-note {\n' +
          '  font-size:\n    var(--type-size-supporting);\n' +
          '  font-weight:\n    var(--type-weight-supporting);\n' +
          '  line-height:\n    var(--type-leading-supporting);\n' +
          '  letter-spacing:\n    var(--type-tracking-supporting);\n' +
          '}\n',
      ),
    ).toEqual([])
  })

  test('camelCase type tokens pass', () => {
    expect(
      violations(
        "const style = {\n" +
          "  fontSize: 'var(--type-size-supporting)',\n" +
          "  fontWeight: 'var(--type-weight-supporting)',\n" +
          "  lineHeight: 'var(--type-leading-supporting)',\n" +
          "  letterSpacing: 'var(--type-tracking-supporting)',\n" +
          '}\n',
        'src/windows/main/Settings.tsx',
      ),
    ).toEqual([])
  })

  test('approved relative authored-content ratios pass', () => {
    expect(
      violations(
        '.dara-markdown figcaption {\n  font-size: 0.78em;\n}\n',
        'src/markdown/markdown-renderer.css',
      ),
    ).toEqual([])
    expect(
      violations(
        '.dara-rich-text-content code {\n  font-size: 0.9em;\n}\n',
        'src/markdown/rich-text-editor.css',
      ),
    ).toEqual([])
  })

  test('unapproved relative ratios fail even in authored-content files', () => {
    expect(
      violations(
        '.dara-markdown h1 {\n  font-size: 1.2em;\n}\n',
        'src/markdown/markdown-renderer.css',
      ),
    ).toHaveLength(1)
    expect(
      violations(
        '.feature-heading {\n  font-size: 1.45em;\n}\n',
        FEATURE_STYLESHEET,
      ),
    ).toHaveLength(1)
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
