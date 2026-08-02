import { readFileSync, readdirSync } from 'node:fs'
import { extname, relative, resolve } from 'node:path'

/*
 * Dara's application typography lives in one place: the type tokens declared in
 * src/styles/base.css and consumed through DaraText or a shared control's own
 * stylesheet. This check fails the build when a feature invents its own font
 * size, weight, tracking, or leading instead of using a named role.
 */

const sourceRoot = resolve('src')
const tokenSource = resolve('src/styles/base.css')

export const TypographyCheckKind = {
  FontSize: 'font-size',
  FontShorthand: 'font shorthand',
  InlineFontSize: 'fontSize',
  FontWeight: 'font-weight',
  LetterSpacing: 'letter-spacing',
  LineHeight: 'line-height',
  TypographyVariable: 'typography variable',
}

const numericLiteralPattern =
  /(^|[^\w$])(?:\d+(?:\.\d*)?|\.\d+)(?:[a-z%]+)?(?![\w$])/i
const cssFontSizeLiteralPattern =
  /(?:\d+(?:\.\d*)?|\.\d+)\s*(?:%|[a-z]+)(?![\w-])/i
const customPropertyReferencePattern = /var\(\s*(--[\w-]+)/g

const wrappedCssCheckByProperty = {
  'font-size': {
    name: TypographyCheckKind.FontSize,
    test: (value) => cssFontSizeLiteralPattern.test(value),
    message: 'font-size uses a raw size literal instead of a type token',
  },
  font: {
    name: TypographyCheckKind.FontShorthand,
    test: (value) => cssFontSizeLiteralPattern.test(value),
    message:
      'font shorthand embeds a raw size; use the separate type token properties',
  },
  'font-weight': {
    name: TypographyCheckKind.FontWeight,
    test: (value) => /^\s*\d/.test(value),
    message: 'font-weight uses a raw numeric value instead of a weight token',
  },
  'letter-spacing': {
    name: TypographyCheckKind.LetterSpacing,
    test: (value) => /^\s*-?\.?\d/.test(value),
    message:
      'letter-spacing uses a raw numeric value instead of a tracking token',
  },
  'line-height': {
    name: TypographyCheckKind.LineHeight,
    test: (value) => /^\s*\d/.test(value),
    message: 'line-height uses a raw numeric value instead of a leading token',
  },
}

const inlineTypographyCheckByProperty = {
  fontSize: {
    name: TypographyCheckKind.InlineFontSize,
    message: 'fontSize assigns a numeric literal instead of a type token',
  },
  fontWeight: {
    name: TypographyCheckKind.FontWeight,
    message: 'fontWeight assigns a numeric literal instead of a weight token',
  },
  letterSpacing: {
    name: TypographyCheckKind.LetterSpacing,
    message:
      'letterSpacing assigns a numeric literal instead of a tracking token',
  },
  lineHeight: {
    name: TypographyCheckKind.LineHeight,
    message: 'lineHeight assigns a numeric literal instead of a leading token',
  },
}

/*
 * Every exception is narrow and carries its reason. Dynamic SVG text is sized
 * from geometry rather than from the type scale, while authored rich content
 * needs ratios that remain relative to its surrounding content role.
 */
const allowlist = [
  {
    file: 'src/windows/main/Home.tsx',
    pattern: /fontSize=\{12 \* ACTIVITY_GRAPH_SCALE\}/,
    reason:
      'react-activity-calendar label size is derived from the graph scale so labels stay proportional to block geometry.',
  },
  {
    file: 'src/occlusion/OcclusionEditor.tsx',
    pattern: /fontSize=\{maskNumberBadgeRadius \* 1\.15\}/,
    reason:
      'Mask number size is derived from the badge radius so the numeral always fits its circle.',
  },
  {
    file: 'src/occlusion/occlusion.css',
    pattern: /^\s*font-weight: 800;$/,
    reason:
      'Mask number weight belongs to the same geometry-derived SVG badge as the OcclusionEditor mask size.',
  },
  {
    file: 'src/markdown/markdown-renderer.css',
    pattern:
      /^\s*font-size: (?:1\.45|1\.28|1\.14|0\.78|0\.84|0\.76|0\.8)em;$/,
    reason:
      'Rendered Markdown hierarchy and inline constructs scale relative to the selected authored-content role.',
  },
  {
    file: 'src/markdown/rich-text-editor.css',
    pattern: /^\s*font-size: 0\.9em;$/,
    reason:
      'Inline code in authored rich text scales relative to the editor content role.',
  },
]

const checks = [
  {
    name: TypographyCheckKind.FontSize,
    // A font-size may use tokens or keywords. Approved authored-content ratios
    // are documented as exact allowlist entries above.
    test: (line) =>
      /(^|[^-\w])font-size\s*:/.test(line) &&
      cssFontSizeLiteralPattern.test(line),
    message: 'font-size uses a raw size literal instead of a type token',
  },
  {
    name: TypographyCheckKind.FontShorthand,
    test: (line) =>
      /(^|[^-\w])font\s*:/.test(line) &&
      cssFontSizeLiteralPattern.test(line),
    message:
      'font shorthand embeds a raw size; use the separate type token properties',
  },
  ...Object.entries(inlineTypographyCheckByProperty).map(
    ([property, check]) => ({
      name: check.name,
      test: (line) => hasNumericTypographyAssignment(line, property),
      message: check.message,
    }),
  ),
  {
    name: TypographyCheckKind.FontWeight,
    test: (line) => /(^|[^-\w])font-weight\s*:\s*\d/.test(line),
    message: 'font-weight uses a raw numeric value instead of a weight token',
  },
  {
    name: TypographyCheckKind.LetterSpacing,
    test: (line) => /(^|[^-\w])letter-spacing\s*:\s*-?\.?\d/.test(line),
    message:
      'letter-spacing uses a raw numeric value instead of a tracking token',
  },
  {
    name: TypographyCheckKind.LineHeight,
    test: (line) => /(^|[^-\w])line-height\s*:\s*\d/.test(line),
    message: 'line-height uses a raw numeric value instead of a leading token',
  },
]

const checkedExtensions = new Set(['.css', '.ts', '.tsx'])

function hasNumericTypographyAssignment(line, property) {
  const assignmentPatterns = [
    new RegExp(`\\b${property}\\b\\s*:\\s*([^,}\\n]+)`, 'g'),
    new RegExp(`\\b${property}\\b\\s*=\\s*\\{([^}]*)\\}`, 'g'),
    new RegExp(`\\b${property}\\b\\s*=\\s*([^;,\\n]+)`, 'g'),
  ]

  return assignmentPatterns.some((pattern) => {
    for (const match of line.matchAll(pattern)) {
      if (numericLiteralPattern.test(match[1])) {
        return true
      }
    }
    return false
  })
}

function referencedCustomProperties(value) {
  return [...value.matchAll(customPropertyReferencePattern)].map(
    (match) => match[1],
  )
}

function lineAtOffset(contents, offset) {
  return contents.slice(0, offset).split('\n').length
}

function sourceForRange(rawLines, startLine, endLine) {
  return rawLines
    .slice(startLine - 1, endLine)
    .map((line) => line.trim())
    .filter(Boolean)
    .join(' ')
}

function findWrappedDeclarationViolations(
  path,
  rawLines,
  analyzedLines,
  allowed,
) {
  const analyzedContents = analyzedLines.join('\n')
  const violations = []

  if (extname(path) === '.css') {
    const declarationPattern =
      /(?:^|[;{])\s*(font-size|font|font-weight|letter-spacing|line-height)\s*:\s*([^;}]+)/gm

    for (const match of analyzedContents.matchAll(declarationPattern)) {
      const propertyEnd = match[0].indexOf(match[1]) + match[1].length
      const valueStart = match[0].lastIndexOf(match[2])
      if (!match[0].slice(propertyEnd, valueStart).includes('\n')) {
        continue
      }

      const check = wrappedCssCheckByProperty[match[1]]
      if (!check.test(match[2])) {
        continue
      }

      const propertyOffset = match.index + match[0].indexOf(match[1])
      const startLine = lineAtOffset(analyzedContents, propertyOffset)
      const endLine = lineAtOffset(analyzedContents, match.index + match[0].length)
      const source = sourceForRange(rawLines, startLine, endLine)
      if (allowed.some((entry) => entry.pattern.test(source))) {
        continue
      }

      violations.push({
        path,
        line: startLine,
        check: check.name,
        message: check.message,
        source,
      })
    }

    return violations
  }

  const inlinePropertyPattern = Object.keys(
    inlineTypographyCheckByProperty,
  ).join('|')
  const inlineAssignmentPattern = new RegExp(
    `\\b(${inlinePropertyPattern})\\b\\s*(?::|=\\s*\\{?)\\s*([\\s\\S]*?)(?=,|;|\\}|\\n\\s*[A-Za-z_$][\\w$]*\\s*:|$)`,
    'g',
  )
  for (const match of analyzedContents.matchAll(inlineAssignmentPattern)) {
    const property = match[1]
    const check = inlineTypographyCheckByProperty[property]
    if (
      !match[0].includes('\n') ||
      hasNumericTypographyAssignment(match[0].split('\n')[0], property) ||
      !numericLiteralPattern.test(match[2])
    ) {
      continue
    }

    const startLine = lineAtOffset(analyzedContents, match.index)
    const endLine = lineAtOffset(analyzedContents, match.index + match[0].length)
    const source = sourceForRange(rawLines, startLine, endLine)
    if (allowed.some((entry) => entry.pattern.test(source))) {
      continue
    }

    violations.push({
      path,
      line: startLine,
      check: check.name,
      message: check.message,
      source,
    })
  }

  return violations
}

function findTypographyVariableViolations(path, rawLines, analyzedLines) {
  if (extname(path) !== '.css') {
    return []
  }

  const analyzedContents = analyzedLines.join('\n')
  const definitions = new Map()
  const definitionPattern =
    /(?:^|[;{])\s*(--[\w-]+)\s*:\s*([^;}]+)/gm
  const typographyDeclarationPattern =
    /(?:^|[;{])\s*(?:font-size|font|font-weight|letter-spacing|line-height)\s*:\s*([^;}]+)/gm

  for (const match of analyzedContents.matchAll(definitionPattern)) {
    const valueStart = match.index + match[0].lastIndexOf(match[2])
    definitions.set(match[1], {
      line: analyzedContents.slice(0, valueStart).split('\n').length,
      value: match[2],
    })
  }

  const pending = []
  for (const match of analyzedContents.matchAll(typographyDeclarationPattern)) {
    pending.push(...referencedCustomProperties(match[1]))
  }

  const visited = new Set()
  const violations = []
  while (pending.length > 0) {
    const name = pending.pop()
    if (visited.has(name)) {
      continue
    }
    visited.add(name)

    const definition = definitions.get(name)
    if (!definition) {
      continue
    }
    pending.push(...referencedCustomProperties(definition.value))

    if (numericLiteralPattern.test(definition.value)) {
      violations.push({
        path,
        line: definition.line,
        check: TypographyCheckKind.TypographyVariable,
        message:
          'a custom property used by typography contains a raw numeric value instead of a type token',
        source: rawLines[definition.line - 1].trim(),
      })
    }
  }

  return violations
}

function linesWithoutComments(rawLines) {
  const analyzedLines = []
  let inBlockComment = false

  for (const rawLine of rawLines) {
    let line = rawLine

    if (inBlockComment) {
      const end = line.indexOf('*/')
      if (end === -1) {
        analyzedLines.push('')
        continue
      }
      line = line.slice(end + 2)
      inBlockComment = false
    }
    const blockStart = line.indexOf('/*')
    if (blockStart !== -1) {
      const end = line.indexOf('*/', blockStart + 2)
      if (end === -1) {
        inBlockComment = true
        line = line.slice(0, blockStart)
      } else {
        line = line.slice(0, blockStart) + line.slice(end + 2)
      }
    }
    const lineComment = line.indexOf('//')
    if (lineComment !== -1 && !/https?:$/.test(line.slice(0, lineComment))) {
      line = line.slice(0, lineComment)
    }
    analyzedLines.push(line)
  }

  return analyzedLines
}

export function findTypographyViolations(path, contents, relativePath) {
  const violations = []
  const allowed = allowlist.filter((entry) => entry.file === relativePath)
  const rawLines = contents.split('\n')
  const analyzedLines = linesWithoutComments(rawLines)

  analyzedLines.forEach((line, index) => {
    const rawLine = rawLines[index]
    if (!line.trim()) {
      return
    }

    if (allowed.some((entry) => entry.pattern.test(rawLine))) {
      return
    }

    for (const check of checks) {
      if (check.test(line)) {
        violations.push({
          path,
          line: index + 1,
          check: check.name,
          message: check.message,
          source: rawLine.trim(),
        })
      }
    }
  })

  violations.push(
    ...findWrappedDeclarationViolations(
      path,
      rawLines,
      analyzedLines,
      allowed,
    ),
    ...findTypographyVariableViolations(path, rawLines, analyzedLines),
  )

  return violations
}

function listSourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name)
    if (entry.isDirectory()) {
      return entry.name === 'node_modules' ? [] : listSourceFiles(path)
    }
    return checkedExtensions.has(extname(entry.name)) ? [path] : []
  })
}

function main() {
  const violations = []
  let checkedFiles = 0

  for (const path of listSourceFiles(sourceRoot)) {
    if (path === tokenSource) {
      continue
    }
    checkedFiles += 1
    violations.push(
      ...findTypographyViolations(
        path,
        readFileSync(path, 'utf8'),
        relative(process.cwd(), path).replaceAll('\\', '/'),
      ),
    )
  }

  if (violations.length > 0) {
    for (const violation of violations) {
      const location = `${relative(process.cwd(), violation.path)}:${violation.line}`
      console.error(`${location}  ${violation.message}\n    ${violation.source}`)
    }
    console.error(
      `\nDefine application typography with the tokens in src/styles/base.css and use DaraText or a shared control's stylesheet. ${violations.length} violation(s).`,
    )
    process.exit(1)
  }

  console.log(
    `Typography contracts passed: ${checkedFiles} source files checked, ${allowlist.length} documented exceptions.`,
  )
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main()
}
