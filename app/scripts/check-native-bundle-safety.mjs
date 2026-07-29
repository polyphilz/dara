import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import process from 'node:process'

const manifest = 'src-tauri/Cargo.toml'
const productionTree = cargoTree([])
const e2eTree = cargoTree(['--features', 'e2e'])
const ordinaryConfigText = readFileSync('src-tauri/tauri.conf.json', 'utf8')
const ordinaryConfig = JSON.parse(ordinaryConfigText)
const releaseConfig = JSON.parse(
  readFileSync('src-tauri/tauri.release.conf.json', 'utf8'),
)
const e2eConfig = JSON.parse(
  readFileSync('src-tauri/tauri.e2e.conf.json', 'utf8'),
)

const localIdentity = Object.freeze({
  productName: 'Dara Local',
  identifier: 'com.rohan.dara.local',
})
const productionIdentity = Object.freeze({
  productName: 'Dara',
  identifier: 'com.rohan.dara',
})
const e2eIdentity = Object.freeze({
  productName: 'Dara E2E',
  identifier: 'com.rohan.dara.e2e',
})

for (const marker of ['tauri-plugin-wdio', 'wdio-webdriver']) {
  if (productionTree.includes(marker)) {
    throw new Error(`Ordinary Cargo graph contains ${marker}`)
  }
  if (
    ordinaryConfigText.includes(marker) ||
    ordinaryConfigText.includes('withGlobalTauri')
  ) {
    throw new Error(`Ordinary Tauri config contains E2E marker ${marker}`)
  }
}
for (const required of ['tauri-plugin-wdio v', 'tauri-plugin-wdio-webdriver v']) {
  if (!e2eTree.includes(required)) {
    throw new Error(`E2E Cargo graph is missing ${required}`)
  }
}

assertIdentity(ordinaryConfig, localIdentity, 'ordinary development config')
assertIdentity(releaseConfig, productionIdentity, 'release config')
assertIdentity(e2eConfig, e2eIdentity, 'native E2E config')
if (ordinaryConfig.app.windows[0]?.title !== localIdentity.productName) {
  throw new Error('Ordinary development window must identify itself as Dara Local')
}

const requiredResources = {
  'resources/release/bin/llama-server': 'bin/llama-server',
  'resources/release/bin/litestream': 'bin/litestream',
  'resources/release/llama-server.json': 'release/llama-server.json',
  'resources/release/litestream.json': 'release/litestream.json',
  'resources/release/licenses/llama.cpp-LICENSE':
    'licenses/llama.cpp-LICENSE',
  'resources/release/licenses/litestream-LICENSE':
    'licenses/litestream-LICENSE',
  'resources/release/licenses/litestream-NOTICE':
    'licenses/litestream-NOTICE',
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
  ordinaryConfig.bundle.resources,
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
  'Native bundle safety passed: development, release, and E2E identities are separate; WDIO plugins are feature-gated; release resources are explicit; and model/test artifacts are excluded.',
)

function assertIdentity(config, expected, label) {
  if (
    config.productName !== expected.productName ||
    config.identifier !== expected.identifier
  ) {
    throw new Error(
      `Unexpected ${label} identity: ${JSON.stringify({
        productName: config.productName,
        identifier: config.identifier,
      })}`,
    )
  }
}

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
