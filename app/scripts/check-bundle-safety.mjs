import { readFileSync, readdirSync } from 'node:fs'
import { extname, resolve } from 'node:path'

const reportPath = resolve('node_modules/.tmp/dara-bundle-report.json')
const report = JSON.parse(readFileSync(reportPath, 'utf8'))
const expectedHtmlEntries = ['index.html', 'quick-add.html']
const prohibitedModuleSegments = [
  'playwright',
  '@wdio/',
  'tauri-plugin-wdio',
]
const prohibitedOutputMarkers = [
  '__DARA_BROWSER_TEST__',
  'playwright-report',
  'test-results',
  '__screenshots__',
  'wdioTauri',
]

assertEqual(
  report.productionHtmlEntries,
  expectedHtmlEntries,
  'production HTML entries',
)

for (const chunk of report.moduleGraph) {
  for (const moduleId of chunk.modules) {
    const normalized = moduleId.replaceAll('\\', '/')
    const prohibited =
      normalized.startsWith('tests/') || normalized.includes('/tests/')
        ? 'tests/'
        : prohibitedModuleSegments.find((segment) =>
            normalized.includes(segment),
          )
    if (prohibited) {
      throw new Error(
        `Production chunk ${chunk.fileName} contains prohibited module ${moduleId}`,
      )
    }
  }
}

for (const path of listFiles(resolve('dist'))) {
  const normalized = path.replaceAll('\\', '/')
  if (
    normalized.includes('/tests/') ||
    normalized.includes('__screenshots__') ||
    normalized.endsWith('.actual.png') ||
    normalized.endsWith('.diff.png')
  ) {
    throw new Error(`Production output contains test artifact ${path}`)
  }
  if (['.html', '.js', '.css', '.json'].includes(extname(path))) {
    const contents = readFileSync(path, 'utf8')
    const marker = prohibitedOutputMarkers.find((value) =>
      contents.includes(value),
    )
    if (marker) {
      throw new Error(`Production output ${path} contains test marker ${marker}`)
    }
  }
}

console.log(
  `Bundle safety passed: ${report.moduleGraph.length} chunks, ${report.files.length} files, ${expectedHtmlEntries.length} production entries.`,
)

function assertEqual(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `Unexpected ${label}: ${JSON.stringify(actual)}; expected ${JSON.stringify(expected)}`,
    )
  }
}

function listFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name)
    return entry.isDirectory() ? listFiles(path) : [path]
  })
}
