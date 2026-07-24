import { gzipSync } from 'node:zlib'
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { dirname, extname, relative, resolve } from 'node:path'
import type { Plugin } from 'vite'

const BundleAssetKind = {
  Css: 'css',
  Font: 'font',
  Html: 'html',
  JavaScript: 'javascript',
  Other: 'other',
} as const

type BundleAssetKind =
  (typeof BundleAssetKind)[keyof typeof BundleAssetKind]

interface BundleModuleChunk {
  facadeModuleId: string | null
  fileName: string
  isEntry: boolean
  modules: string[]
}

const fontExtensions = new Set(['.otf', '.ttf', '.woff', '.woff2'])

export function bundleReportPlugin(): Plugin {
  const reportPath = process.env.DARA_BUNDLE_REPORT_PATH
  let moduleGraph: BundleModuleChunk[] = []

  return {
    name: 'dara-bundle-report',
    generateBundle(_options, bundle) {
      moduleGraph = Object.values(bundle)
        .filter((output) => output.type === 'chunk')
        .map((chunk) => ({
          facadeModuleId:
            chunk.facadeModuleId === null
              ? null
              : normalizeModuleId(chunk.facadeModuleId),
          fileName: chunk.fileName,
          isEntry: chunk.isEntry,
          modules: Object.keys(chunk.modules).map(normalizeModuleId).sort(),
        }))
        .sort((left, right) => left.fileName.localeCompare(right.fileName))
    },
    closeBundle() {
      if (!reportPath) {
        return
      }
      const root = process.cwd()
      const outputDirectory = resolve(root, 'dist')
      const files = listFiles(outputDirectory).map((path) => {
        const bytes = readFileSync(path)
        return {
          fileName: relative(outputDirectory, path),
          gzipBytes: gzipSync(bytes).byteLength,
          kind: assetKind(path),
          rawBytes: bytes.byteLength,
        }
      })
      const kinds = Object.values(BundleAssetKind)
      const totals = Object.fromEntries(
        kinds.map((kind) => {
          const matching = files.filter((file) => file.kind === kind)
          return [
            kind,
            {
              files: matching.length,
              gzipBytes: matching.reduce((total, file) => total + file.gzipBytes, 0),
              rawBytes: matching.reduce((total, file) => total + file.rawBytes, 0),
            },
          ]
        }),
      )
      const resolvedReportPath = resolve(root, reportPath)
      mkdirSync(dirname(resolvedReportPath), { recursive: true })
      writeFileSync(
        resolvedReportPath,
        `${JSON.stringify({
          schemaVersion: 1,
          productionHtmlEntries: files
            .filter((file) => file.kind === BundleAssetKind.Html)
            .map((file) => file.fileName)
            .sort(),
          moduleGraph,
          files,
          totals,
        }, null, 2)}\n`,
      )
    },
  }
}

function normalizeModuleId(moduleId: string): string {
  const root = `${process.cwd()}/`
  return moduleId.startsWith(root) ? moduleId.slice(root.length) : moduleId
}

function listFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const path = resolve(directory, entry.name)
      return entry.isDirectory() ? listFiles(path) : [path]
    })
    .sort()
}

function assetKind(path: string): BundleAssetKind {
  const extension = extname(path).toLowerCase()
  if (extension === '.js' || extension === '.mjs') {
    return BundleAssetKind.JavaScript
  }
  if (extension === '.css') {
    return BundleAssetKind.Css
  }
  if (extension === '.html') {
    return BundleAssetKind.Html
  }
  if (fontExtensions.has(extension)) {
    return BundleAssetKind.Font
  }
  return BundleAssetKind.Other
}
