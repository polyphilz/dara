import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  readdirSync,
} from 'node:fs'
import { extname, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const localTauriConfig = readJson('src-tauri/tauri.conf.json')
const tauriConfig = readJson('src-tauri/tauri.release.conf.json')
const applicationIdentities = readJson('src-tauri/app-identities.json')
const ApplicationIdentityKey = Object.freeze({
  Local: 'local',
  Production: 'production',
})
const llamaPin = readJson(
  'src-tauri/resources/sidecars/llama-server-v1.json',
)
const litestreamPin = readJson(
  'src-tauri/resources/sidecars/litestream-v1.json',
)
const stage = resolve('src-tauri/resources/release')
const sourceProvenancePath = resolve(stage, 'source.json')
const sourceProvenance = readJson(sourceProvenancePath)
const releaseManifestPath = resolve(
  stage,
  llamaPin.stagingPaths.releaseManifest,
)
const releaseManifest = JSON.parse(readFileSync(releaseManifestPath, 'utf8'))
const binaryPath = resolve(stage, llamaPin.stagingPaths.binary)
const license = releaseManifest.licenseNotices[0]
const licensePath = resolve(stage, llamaPin.stagingPaths.license)

assertEqual(
  {
    productName: localTauriConfig.productName,
    identifier: localTauriConfig.identifier,
  },
  currentIdentity(applicationIdentities[ApplicationIdentityKey.Local]),
  'local application identity',
)
assertEqual(
  {
    productName: tauriConfig.productName,
    identifier: tauriConfig.identifier,
  },
  currentIdentity(applicationIdentities[ApplicationIdentityKey.Production]),
  'release application identity',
)

assertEqual(
  stripGeneratedFields(releaseManifest),
  llamaPin,
  'release inputs',
)
assertEqual(
  sourceProvenance,
  {
    commit: run('git', ['rev-parse', 'HEAD'], { capture: true }),
    dirty: run('git', ['status', '--porcelain']).length > 0,
  },
  'release source provenance',
)
assertEqual(releaseManifest.target.architecture, 'arm64', 'architecture')
assertEqual(releaseManifest.verification.modelBundled, false, 'model policy')
assertEqual(releaseManifest.verification.cpuPassed, true, 'CPU verification')
assertEqual(releaseManifest.verification.metalPassed, true, 'Metal verification')

const binaryMetadata = lstatSync(binaryPath)
assert(binaryMetadata.isFile(), 'staged llama-server is not a regular file')
assert(!binaryMetadata.isSymbolicLink(), 'staged llama-server must not be a symlink')
assert(
  (binaryMetadata.mode & 0o111) !== 0,
  'staged llama-server is not executable',
)
assertEqual(
  binaryMetadata.size,
  releaseManifest.binary.size,
  'binary byte length',
)
assertEqual(
  sha256File(binaryPath),
  releaseManifest.binary.sha256,
  'binary SHA-256',
)

const fileDescription = run('file', ['-b', binaryPath])
assert(
  fileDescription.includes('Mach-O 64-bit executable arm64'),
  `unexpected binary format: ${fileDescription}`,
)

const dependencies = run('otool', ['-L', binaryPath])
  .split('\n')
  .slice(1)
  .map((line) => line.trim())
  .filter(Boolean)
const nonSystemDependency = dependencies.find(
  (dependency) =>
    !dependency.startsWith('/System/Library/') &&
    !dependency.startsWith('/usr/lib/'),
)
assert(
  !nonSystemDependency,
  `llama-server has a non-system dependency: ${nonSystemDependency}`,
)

assertEqual(
  run(binaryPath, ['--version']),
  releaseManifest.binary.versionOutput,
  'llama-server version output',
)
assertEqual(
  sha256File('src-tauri/resources/embedding-indexes/jina-v1.json'),
  releaseManifest.verification.embeddingManifest.sha256,
  'embedding manifest SHA-256',
)
assertEqual(
  sha256File('src-tauri/resources/embedding-indexes/jina-v1-golden.json'),
  releaseManifest.verification.goldenFixtures.sha256,
  'golden fixtures SHA-256',
)
assertEqual(sha256File(licensePath), license.sha256, 'license SHA-256')

const litestreamManifestPath = resolve(
  stage,
  litestreamPin.stagingPaths.releaseManifest,
)
const litestreamManifest = readJson(litestreamManifestPath)
const litestreamBinaryPath = resolve(
  stage,
  litestreamPin.stagingPaths.binary,
)
const litestreamLicensePath = resolve(
  stage,
  litestreamPin.stagingPaths.license,
)
const litestreamNoticePath = resolve(
  stage,
  litestreamPin.stagingPaths.notice,
)
assertEqual(litestreamManifest, litestreamPin, 'Litestream release manifest')
assertEqual(
  litestreamPin.verification.requiredL0Retention,
  '720h',
  'Litestream exact-TXID L0 retention',
)
for (const [name, value] of Object.entries(litestreamPin.verification)) {
  if (typeof value === 'boolean') {
    assert(value, `Litestream verification did not pass: ${name}`)
  }
}

const litestreamMetadata = lstatSync(litestreamBinaryPath)
assert(
  litestreamMetadata.isFile(),
  'staged Litestream is not a regular file',
)
assert(
  !litestreamMetadata.isSymbolicLink(),
  'staged Litestream must not be a symlink',
)
assert(
  (litestreamMetadata.mode & 0o111) !== 0,
  'staged Litestream is not executable',
)
assertEqual(
  litestreamMetadata.size,
  litestreamPin.binary.size,
  'Litestream binary byte length',
)
assertEqual(
  sha256File(litestreamBinaryPath),
  litestreamPin.binary.sha256,
  'Litestream binary SHA-256',
)

const litestreamDescription = run('file', ['-b', litestreamBinaryPath])
assert(
  litestreamDescription.includes('Mach-O 64-bit executable arm64'),
  `unexpected Litestream binary format: ${litestreamDescription}`,
)
const litestreamDependencies = run('otool', ['-L', litestreamBinaryPath])
  .split('\n')
  .slice(1)
  .map((line) => line.trim())
  .filter(Boolean)
const litestreamNonSystemDependency = litestreamDependencies.find(
  (dependency) =>
    !dependency.startsWith('/System/Library/') &&
    !dependency.startsWith('/usr/lib/'),
)
assert(
  !litestreamNonSystemDependency,
  `Litestream has a non-system dependency: ${litestreamNonSystemDependency}`,
)
assertEqual(
  run(litestreamBinaryPath, litestreamPin.binary.versionArguments),
  litestreamPin.binary.versionOutput,
  'Litestream version output',
)
assertEqual(
  sha256File(litestreamLicensePath),
  litestreamPin.licenseNotices[0].sha256,
  'Litestream license SHA-256',
)
assertEqual(
  readFileSync(litestreamNoticePath, 'utf8'),
  readFileSync(
    'src-tauri/resources/sidecars/litestream-NOTICE',
    'utf8',
  ),
  'Litestream notice',
)

const requiredResources = {
  'resources/release/bin/llama-server': 'bin/llama-server',
  'resources/release/bin/litestream': 'bin/litestream',
  'resources/release/llama-server.json': 'release/llama-server.json',
  'resources/release/litestream.json': 'release/litestream.json',
  'resources/release/source.json': 'release/source.json',
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
assertEqual(
  tauriConfig.bundle.resources,
  requiredResources,
  'Tauri release resources',
)
assertEqual(
  tauriConfig.bundle.macOS.minimumSystemVersion,
  llamaPin.target.minimumSystemVersion,
  'minimum macOS version',
)
assertEqual(
  tauriConfig.bundle.macOS.minimumSystemVersion,
  litestreamPin.target.minimumSystemVersion,
  'Litestream minimum macOS version',
)
assertEqual(tauriConfig.bundle.macOS.signingIdentity, '-', 'signing identity')
assertEqual(
  tauriConfig.bundle.macOS.hardenedRuntime,
  false,
  'personal-v1 hardened runtime policy',
)

for (const path of listFiles(stage)) {
  const normalized = path.replaceAll('\\', '/')
  assert(
    extname(path) !== '.gguf',
    `model weights must not be bundled: ${normalized}`,
  )
  assert(
    !normalized.includes('/tests/') &&
      !normalized.includes('playwright') &&
      !normalized.includes('wdio'),
    `test artifact must not be bundled: ${normalized}`,
  )
}

console.info(
  `Release resources passed: llama-server ${releaseManifest.binary.sha256} (${releaseManifest.binary.size} bytes), Litestream ${litestreamPin.binary.sha256} (${litestreamPin.binary.size} bytes), arm64 with protocol gates verified.`,
)

function stripGeneratedFields(manifest) {
  const { binary: _binary, verification: _verification, ...inputs } = manifest
  return {
    ...inputs,
    licenseNotices: inputs.licenseNotices.map(
      ({ sha256: _sha256, ...notice }) => notice,
    ),
  }
}

function currentIdentity(identity) {
  return {
    productName: identity.productName,
    identifier: identity.identifier,
  }
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function run(command, arguments_) {
  const result = spawnSync(command, arguments_, { encoding: 'utf8' })
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.stderr.write(result.stdout)
    process.stderr.write(result.stderr)
    process.exit(result.status ?? 1)
  }
  return `${result.stdout}${result.stderr}`.trim()
}

function listFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name)
    return entry.isDirectory() ? listFiles(path) : [path]
  })
}

function assertEqual(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `Unexpected ${label}: ${JSON.stringify(actual)}; expected ${JSON.stringify(expected)}`,
    )
  }
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}
