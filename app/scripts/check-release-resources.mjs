import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  readdirSync,
} from 'node:fs'
import { extname, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const tauriConfig = readJson('src-tauri/tauri.release.conf.json')
const pin = readJson(
  'src-tauri/resources/sidecars/llama-server-v1.json',
)
const stage = resolve('src-tauri/resources/release')
const releaseManifestPath = resolve(
  stage,
  pin.stagingPaths.releaseManifest,
)
const releaseManifest = JSON.parse(readFileSync(releaseManifestPath, 'utf8'))
const binaryPath = resolve(stage, pin.stagingPaths.binary)
const license = releaseManifest.licenseNotices[0]
const licensePath = resolve(stage, pin.stagingPaths.license)

assertEqual(
  stripGeneratedFields(releaseManifest),
  pin,
  'release inputs',
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
assertEqual(
  tauriConfig.bundle.resources,
  requiredResources,
  'Tauri release resources',
)
assertEqual(
  tauriConfig.bundle.macOS.minimumSystemVersion,
  pin.target.minimumSystemVersion,
  'minimum macOS version',
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
  `Release resources passed: llama-server ${releaseManifest.binary.sha256} (${releaseManifest.binary.size} bytes), arm64, CPU + Metal verified.`,
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
