import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  readdirSync,
} from 'node:fs'
import { basename, extname, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const appPath = resolve(
  process.argv[2] ?? 'src-tauri/target/release/bundle/macos/Dara.app',
)
const resources = resolve(appPath, 'Contents/Resources')
const appBinary = resolve(appPath, 'Contents/MacOS/dara')
const sidecar = resolve(resources, 'bin/llama-server')
const releaseManifestPath = resolve(resources, 'release/llama-server.json')
const releaseManifest = JSON.parse(readFileSync(releaseManifestPath, 'utf8'))
const pin = JSON.parse(
  readFileSync(
    'src-tauri/resources/sidecars/llama-server-v1.json',
    'utf8',
  ),
)

assert(lstatSync(appPath).isDirectory(), 'Dara.app is not a directory')
assert(lstatSync(appBinary).isFile(), 'Dara executable is missing')
assert(lstatSync(sidecar).isFile(), 'bundled llama-server is missing')
assert(
  (lstatSync(sidecar).mode & 0o111) !== 0,
  'bundled llama-server is not executable',
)

assertEqual(
  sha256File(sidecar),
  releaseManifest.binary.sha256,
  'bundled llama-server SHA-256',
)
assertEqual(
  sha256File('src-tauri/resources/release/bin/llama-server'),
  releaseManifest.binary.sha256,
  'staged llama-server SHA-256',
)
assertEqual(
  lstatSync(sidecar).size,
  releaseManifest.binary.size,
  'bundled llama-server size',
)
assertEqual(
  sha256File(resolve(resources, 'embedding-indexes/jina-v1.json')),
  releaseManifest.verification.embeddingManifest.sha256,
  'bundled embedding manifest SHA-256',
)
assertEqual(
  sha256File(resolve(resources, 'embedding-indexes/jina-v1-golden.json')),
  releaseManifest.verification.goldenFixtures.sha256,
  'bundled golden fixtures SHA-256',
)
assertEqual(
  sha256File(resolve(resources, releaseManifest.licenseNotices[0].bundlePath)),
  releaseManifest.licenseNotices[0].sha256,
  'bundled license SHA-256',
)

for (const binary of [appBinary, sidecar]) {
  const description = run('file', ['-b', binary])
  assert(
    description.includes('Mach-O 64-bit executable arm64'),
    `unexpected binary format for ${basename(binary)}: ${description}`,
  )
}

const dependencies = run('otool', ['-L', sidecar])
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
  `bundled llama-server has a non-system dependency: ${nonSystemDependency}`,
)

run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', appPath])
run('codesign', ['--verify', '--strict', '--verbose=2', sidecar])
const signature = run('codesign', ['-dv', '--verbose=4', appPath])
assert(signature.includes('Identifier=com.rohan.dara'), 'unexpected app identifier')
assert(signature.includes('Signature=adhoc'), 'app is not ad-hoc signed')

assertEqual(
  run('/usr/libexec/PlistBuddy', [
    '-c',
    'Print :LSMinimumSystemVersion',
    resolve(appPath, 'Contents/Info.plist'),
  ]),
  pin.target.minimumSystemVersion,
  'packaged minimum macOS version',
)

const quarantine = spawnSync(
  'xattr',
  ['-p', 'com.apple.quarantine', appPath],
  { encoding: 'utf8' },
)
assert(
  quarantine.status !== 0,
  'locally built Dara.app unexpectedly has a quarantine attribute',
)

const allowedResourceFiles = [
  'bin/llama-server',
  'embedding-indexes/jina-v1-golden.json',
  'embedding-indexes/jina-v1.json',
  'icon.icns',
  'licenses/llama.cpp-LICENSE',
  'release/llama-server.json',
]
const packagedResourceFiles = listFiles(resources)
  .map((path) => path.slice(resources.length + 1).replaceAll('\\', '/'))
  .sort()
assertEqual(
  packagedResourceFiles,
  allowedResourceFiles,
  'packaged resource files',
)
for (const path of packagedResourceFiles) {
  const normalized = path.toLowerCase()
  assert(extname(path) !== '.gguf', `model weights were bundled: ${path}`)
  for (const prohibited of ['tests/', 'playwright', 'wdio', 'tauri.e2e']) {
    assert(
      !normalized.includes(prohibited),
      `test artifact was bundled: ${path}`,
    )
  }
}

console.info(
  `Packaged app passed: ${appPath}, ad-hoc signed arm64 macOS ${pin.target.minimumSystemVersion}+, pinned llama-server ${releaseManifest.binary.sha256}.`,
)

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
