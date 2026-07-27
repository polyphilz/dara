import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import process from 'node:process'

const manifest = 'src-tauri/Cargo.toml'
const productionTree = cargoTree([])
const e2eTree = cargoTree(['--features', 'e2e'])
const productionConfig = readFileSync('src-tauri/tauri.conf.json', 'utf8')
const parsedProductionConfig = JSON.parse(productionConfig)
const releaseConfig = JSON.parse(
  readFileSync('src-tauri/tauri.release.conf.json', 'utf8'),
)

for (const marker of ['tauri-plugin-wdio', 'wdio-webdriver']) {
  if (productionTree.includes(marker)) {
    throw new Error(`Ordinary Cargo graph contains ${marker}`)
  }
  if (productionConfig.includes(marker) || productionConfig.includes('withGlobalTauri')) {
    throw new Error(`Ordinary Tauri config contains E2E marker ${marker}`)
  }
}
for (const required of ['tauri-plugin-wdio v', 'tauri-plugin-wdio-webdriver v']) {
  if (!e2eTree.includes(required)) {
    throw new Error(`E2E Cargo graph is missing ${required}`)
  }
}

const requiredResources = {
  'resources/release/bin/llama-server': 'bin/llama-server',
  'resources/release/llama-server.json': 'release/llama-server.json',
  'resources/release/licenses/llama.cpp-LICENSE':
    'licenses/llama.cpp-LICENSE',
  'resources/embedding-indexes/jina-v1.json':
    'embedding-indexes/jina-v1.json',
  'resources/embedding-indexes/jina-v1-golden.json':
    'embedding-indexes/jina-v1-golden.json',
}
if (
  JSON.stringify(releaseConfig.bundle.resources) !==
  JSON.stringify(requiredResources)
) {
  throw new Error(
    `Unexpected release resources: ${JSON.stringify(releaseConfig.bundle.resources)}`,
  )
}
for (const [source, destination] of Object.entries(requiredResources)) {
  const normalized = `${source}/${destination}`.toLowerCase()
  for (const prohibited of [
    'tests/',
    'playwright',
    'wdio',
    '.gguf',
    'tauri.e2e',
  ]) {
    if (normalized.includes(prohibited)) {
      throw new Error(
        `Production resource ${source} -> ${destination} contains ${prohibited}`,
      )
    }
  }
}
for (const [source, destination] of Object.entries(
  parsedProductionConfig.bundle.resources,
)) {
  if (
    !Object.hasOwn(requiredResources, source) ||
    requiredResources[source] !== destination
  ) {
    throw new Error(
      `Ordinary Tauri config contains a resource absent from the release config: ${source} -> ${destination}`,
    )
  }
}
if (
  releaseConfig.bundle.macOS.minimumSystemVersion !== '14.0' ||
  releaseConfig.bundle.macOS.signingIdentity !== '-' ||
  releaseConfig.bundle.macOS.hardenedRuntime !== false
) {
  throw new Error('Release config must produce an ad-hoc-signed macOS 14 app')
}

console.info(
  'Native bundle safety passed: WDIO plugins are feature-gated out of the ordinary Cargo graph, release resources are explicit, and model/test artifacts are excluded.',
)

function cargoTree(extraArguments) {
  const result = spawnSync(
    'cargo',
    ['tree', '--locked', '--manifest-path', manifest, '-e', 'normal', ...extraArguments],
    { encoding: 'utf8' },
  )
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.stderr.write(result.stderr)
    process.exit(result.status ?? 1)
  }
  return result.stdout
}
