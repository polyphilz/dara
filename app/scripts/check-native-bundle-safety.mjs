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
const distributionSigning = Object.freeze(
  JSON.parse(readFileSync('src-tauri/distribution-signing.json', 'utf8')),
)
const packageJson = JSON.parse(readFileSync('package.json', 'utf8'))
const e2eConfig = JSON.parse(
  readFileSync('src-tauri/tauri.e2e.conf.json', 'utf8'),
)
const applicationIdentities = Object.freeze(
  JSON.parse(readFileSync('src-tauri/app-identities.json', 'utf8')),
)
const ApplicationIdentityKey = Object.freeze({
  Local: 'local',
  Production: 'production',
  E2e: 'e2e',
})
const expectedIdentityKeys = Object.values(ApplicationIdentityKey).sort()
const observedIdentityKeys = Object.keys(applicationIdentities).sort()
if (
  JSON.stringify(observedIdentityKeys) !== JSON.stringify(expectedIdentityKeys)
) {
  throw new Error(
    `Unexpected application identity keys: ${JSON.stringify(observedIdentityKeys)}`,
  )
}
const localIdentity = applicationIdentities[ApplicationIdentityKey.Local]
const productionIdentity =
  applicationIdentities[ApplicationIdentityKey.Production]
const e2eIdentity = applicationIdentities[ApplicationIdentityKey.E2e]
const claimedIdentifiers = new Map()

for (const [identityKey, identity] of Object.entries(applicationIdentities)) {
  assertIdentityShape(identity, identityKey)
  claimIdentifier(identity.identifier, `${identityKey} current identifier`)
  for (const legacyIdentifier of identity.legacyIdentifiers) {
    if (legacyIdentifier === identity.identifier) {
      throw new Error(
        `${identityKey} current identifier must not also be a legacy identifier`,
      )
    }
    claimIdentifier(legacyIdentifier, `${identityKey} legacy identifier`)
  }
}

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
if (
  distributionSigning.formatVersion !== 1 ||
  distributionSigning.application.bundleIdentifier !==
    productionIdentity.identifier ||
  distributionSigning.application.signingIdentity !==
    'Developer ID Application: SILO77 LLC (PMZH6ULML8)' ||
  distributionSigning.application.teamIdentifier !== 'PMZH6ULML8'
) {
  throw new Error('Unexpected Developer ID distribution signing policy')
}
if (
  distributionSigning.sidecars.llamaServer.bundlePath !==
    requiredResources['resources/release/bin/llama-server'] ||
  distributionSigning.sidecars.litestream.bundlePath !==
    requiredResources['resources/release/bin/litestream']
) {
  throw new Error('Distribution sidecar paths diverge from release resources')
}
for (const command of [
  'release:verify-contracts',
  'release:stage-sidecars',
  'release:verify-resources',
  'release:sign-sidecars',
  'release:verify-distribution',
]) {
  const scripts = JSON.stringify(packageJson.scripts)
  if (!scripts.includes(command)) {
    throw new Error(`Distribution release scripts omit ${command}`)
  }
}

console.info(
  'Native bundle safety passed: development, release, and E2E identities are separate; WDIO plugins are feature-gated; release resources are explicit; public distribution has a fixed Developer ID policy; and model/test artifacts are excluded.',
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

function assertIdentityShape(identity, identityKey) {
  if (
    typeof identity?.productName !== 'string' ||
    identity.productName.length === 0 ||
    typeof identity.identifier !== 'string' ||
    !isBundleIdentifier(identity.identifier) ||
    !Array.isArray(identity.legacyIdentifiers) ||
    identity.legacyIdentifiers.some(
      (identifier) => !isBundleIdentifier(identifier),
    )
  ) {
    throw new Error(
      `Invalid ${identityKey} application identity: ${JSON.stringify(identity)}`,
    )
  }
}

function claimIdentifier(identifier, label) {
  const existing = claimedIdentifiers.get(identifier)
  if (existing !== undefined) {
    throw new Error(
      `Application identifier ${identifier} is shared by ${existing} and ${label}`,
    )
  }
  claimedIdentifiers.set(identifier, label)
}

function isBundleIdentifier(identifier) {
  return (
    typeof identifier === 'string' &&
    /^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$/.test(identifier)
  )
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
