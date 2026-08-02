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

/*
 * Every exception is narrow and carries its reason. Dynamic SVG text is sized
 * from geometry rather than from the type scale, so it cannot be tokenized
 * without breaking the drawing it belongs to.
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
]

const checks = [
  {
    name: 'font-size',
    // A font-size may use tokens, relative content ratios, or keywords, but a
    // raw px/rem literal is a new one-off size decision.
    test: (line) =>
      /(^|[^-\w])font-size\s*:/.test(line) && /\d*\.?\d+\s*(px|rem)\b/.test(line),
    message: 'font-size uses a raw px/rem literal instead of a type token',
  },
  {
    name: 'font shorthand',
    test: (line) =>
      /(^|[^-\w])font\s*:/.test(line) && /\d*\.?\d+\s*(px|rem)\b/.test(line),
    message:
      'font shorthand embeds a raw px/rem size; use the separate type token properties',
  },
  {
    name: 'fontSize',
    test: (line) =>
      /\bfontSize\b\s*[:=]/.test(line) &&
      (/\bfontSize\b\s*[:=]\s*['"`]?\d/.test(line) ||
        /\bfontSize=\{[^}]*\d/.test(line)),
    message: 'fontSize assigns a numeric literal instead of a type token',
  },
  {
    name: 'font-weight',
    test: (line) => /(^|[^-\w])font-weight\s*:\s*\d/.test(line),
    message: 'font-weight uses a raw numeric value instead of a weight token',
  },
  {
    name: 'letter-spacing',
    test: (line) => /(^|[^-\w])letter-spacing\s*:\s*-?\.?\d/.test(line),
    message:
      'letter-spacing uses a raw numeric value instead of a tracking token',
  },
  {
    name: 'line-height',
    test: (line) => /(^|[^-\w])line-height\s*:\s*\d/.test(line),
    message: 'line-height uses a raw numeric value instead of a leading token',
  },
]

const checkedExtensions = new Set(['.css', '.ts', '.tsx'])

export function findTypographyViolations(path, contents, relativePath) {
  const violations = []
  const allowed = allowlist.filter((entry) => entry.file === relativePath)
  let inBlockComment = false

  contents.split('\n').forEach((rawLine, index) => {
    let line = rawLine

    // Strip comments so documentation and reasons never trip the checker.
    if (inBlockComment) {
      const end = line.indexOf('*/')
      if (end === -1) {
        return
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
