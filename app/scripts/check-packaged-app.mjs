import { createHash } from 'node:crypto'
import {
  lstatSync,
  readFileSync,
  readdirSync,
} from 'node:fs'
import { basename, extname, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

import {
  DistributionSidecarKey,
  readDistributionSigningPolicy,
  verifyDeveloperIdSignature,
} from './distribution-signing.mjs'

const PackageSignatureMode = Object.freeze({
  AdHoc: 'ad-hoc',
  DeveloperId: 'developer-id',
})

const appPath = resolve(
  process.argv[2] ?? 'src-tauri/target/release/bundle/macos/Dara.app',
)
const signatureMode = process.argv[3] ?? PackageSignatureMode.AdHoc
assert(
  Object.values(PackageSignatureMode).includes(signatureMode),
  `unknown package signature mode: ${signatureMode}`,
)
const distributionPolicy =
  signatureMode === PackageSignatureMode.DeveloperId
    ? readDistributionSigningPolicy()
    : undefined
const resources = resolve(appPath, 'Contents/Resources')
const appBinary = resolve(appPath, 'Contents/MacOS/dara')
const sidecar = resolve(resources, 'bin/llama-server')
const litestream = resolve(resources, 'bin/litestream')
const stagedSidecar = resolve(
  'src-tauri/resources/release/bin/llama-server',
)
const stagedLitestream = resolve(
  'src-tauri/resources/release/bin/litestream',
)
const releaseManifestPath = resolve(resources, 'release/llama-server.json')
const releaseManifest = JSON.parse(readFileSync(releaseManifestPath, 'utf8'))
const applicationIdentities = JSON.parse(
  readFileSync('src-tauri/app-identities.json', 'utf8'),
)
const ApplicationIdentityKey = Object.freeze({
  Production: 'production',
})
const productionIdentity =
  applicationIdentities[ApplicationIdentityKey.Production]
const pin = JSON.parse(
  readFileSync(
    'src-tauri/resources/sidecars/llama-server-v1.json',
    'utf8',
  ),
)
const litestreamManifest = JSON.parse(
  readFileSync(resolve(resources, 'release/litestream.json'), 'utf8'),
)
const sourceProvenance = JSON.parse(
  readFileSync(resolve(resources, 'release/source.json'), 'utf8'),
)
const stagedSourceProvenance = JSON.parse(
  readFileSync('src-tauri/resources/release/source.json', 'utf8'),
)
const litestreamPin = JSON.parse(
  readFileSync(
    'src-tauri/resources/sidecars/litestream-v1.json',
    'utf8',
  ),
)

assert(lstatSync(appPath).isDirectory(), 'Dara.app is not a directory')
assert(lstatSync(appBinary).isFile(), 'Dara executable is missing')
assert(lstatSync(sidecar).isFile(), 'bundled llama-server is missing')
assert(lstatSync(litestream).isFile(), 'bundled Litestream is missing')
assert(
  (lstatSync(sidecar).mode & 0o111) !== 0,
  'bundled llama-server is not executable',
)
assert(
  (lstatSync(litestream).mode & 0o111) !== 0,
  'bundled Litestream is not executable',
)

if (signatureMode === PackageSignatureMode.AdHoc) {
  assertEqual(
    sha256File(sidecar),
    releaseManifest.binary.sha256,
    'bundled llama-server SHA-256',
  )
  assertEqual(
    sha256File(stagedSidecar),
    releaseManifest.binary.sha256,
    'staged llama-server SHA-256',
  )
  assertEqual(
    lstatSync(sidecar).size,
    releaseManifest.binary.size,
    'bundled llama-server size',
  )
}
assertEqual(
  litestreamManifest,
  litestreamPin,
  'bundled Litestream release manifest',
)
assertEqual(
  sourceProvenance,
  stagedSourceProvenance,
  'bundled release source provenance',
)
if (signatureMode === PackageSignatureMode.AdHoc) {
  assertEqual(
    sha256File(litestream),
    litestreamPin.binary.sha256,
    'bundled Litestream SHA-256',
  )
  assertEqual(
    sha256File(stagedLitestream),
    litestreamPin.binary.sha256,
    'staged Litestream SHA-256',
  )
  assertEqual(
    lstatSync(litestream).size,
    litestreamPin.binary.size,
    'bundled Litestream size',
  )
}
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
assertEqual(
  sha256File(
    resolve(resources, litestreamPin.licenseNotices[0].bundlePath),
  ),
  litestreamPin.licenseNotices[0].sha256,
  'bundled Litestream license SHA-256',
)
assertEqual(
  readFileSync(resolve(resources, litestreamPin.resourceDestinations.notice), 'utf8'),
  readFileSync('src-tauri/resources/sidecars/litestream-NOTICE', 'utf8'),
  'bundled Litestream notice',
)

run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', appPath])
run('codesign', ['--verify', '--strict', '--verbose=2', sidecar])
run('codesign', ['--verify', '--strict', '--verbose=2', litestream])
const signature = run('codesign', ['-dv', '--verbose=4', appPath])
assertEqual(
  signatureField(signature, 'Identifier'),
  productionIdentity.identifier,
  'signed application identifier',
)
if (signatureMode === PackageSignatureMode.AdHoc) {
  assertEqual(signatureField(signature, 'Signature'), 'adhoc', 'app signature')
} else {
  verifyDeveloperIdSignature(
    appPath,
    distributionPolicy,
    productionIdentity.identifier,
  )
  for (const [key, path] of [
    [DistributionSidecarKey.LlamaServer, sidecar],
    [DistributionSidecarKey.Litestream, litestream],
  ]) {
    verifyDeveloperIdSignature(
      path,
      distributionPolicy,
      distributionPolicy.sidecars[key].identifier,
    )
  }
}

for (const binary of [appBinary, sidecar, litestream]) {
  const description = run('file', ['-b', binary])
  assert(
    description.includes('Mach-O 64-bit executable arm64'),
    `unexpected binary format for ${basename(binary)}: ${description}`,
  )
}

for (const binary of [sidecar, litestream]) {
  const dependencies = run('otool', ['-L', binary])
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
    `bundled ${basename(binary)} has a non-system dependency: ${nonSystemDependency}`,
  )
}

assertEqual(
  run(litestream, litestreamPin.binary.versionArguments),
  litestreamPin.binary.versionOutput,
  'bundled Litestream version output',
)
assertEqual(
  run('/usr/libexec/PlistBuddy', [
    '-c',
    'Print :CFBundleIdentifier',
    resolve(appPath, 'Contents/Info.plist'),
  ]),
  productionIdentity.identifier,
  'packaged application identifier',
)

assertEqual(
  run('/usr/libexec/PlistBuddy', [
    '-c',
    'Print :LSMinimumSystemVersion',
    resolve(appPath, 'Contents/Info.plist'),
  ]),
  pin.target.minimumSystemVersion,
  'packaged minimum macOS version',
)
assertEqual(
  run('/usr/libexec/PlistBuddy', [
    '-c',
    'Print :LSMinimumSystemVersion',
    resolve(appPath, 'Contents/Info.plist'),
  ]),
  litestreamPin.target.minimumSystemVersion,
  'packaged Litestream minimum macOS version',
)

if (signatureMode === PackageSignatureMode.AdHoc) {
  const quarantine = spawnSync(
    'xattr',
    ['-p', 'com.apple.quarantine', appPath],
    { encoding: 'utf8' },
  )
  assert(
    quarantine.status !== 0,
    'locally built Dara.app unexpectedly has a quarantine attribute',
  )
}

const allowedResourceFiles = [
  'bin/llama-server',
  'bin/litestream',
  'embedding-indexes/jina-v1-golden.json',
  'embedding-indexes/jina-v1.json',
  'icon.icns',
  'licenses/llama.cpp-LICENSE',
  'licenses/litestream-LICENSE',
  'licenses/litestream-NOTICE',
  'release/llama-server.json',
  'release/litestream.json',
  'release/source.json',
].sort()
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

const signatureDescription =
  signatureMode === PackageSignatureMode.AdHoc
    ? 'ad-hoc signed with exact upstream sidecar hashes'
    : `Developer ID signed by ${distributionPolicy.application.teamIdentifier} with hardened sidecars`
console.info(
  `Packaged app passed: ${appPath}, ${signatureDescription}, arm64 macOS ${pin.target.minimumSystemVersion}+, pinned llama-server inputs and Litestream ${litestreamPin.binary.sha256}.`,
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

function signatureField(signature, field) {
  const prefix = `${field}=`
  const values = signature
    .split(/\r?\n/u)
    .filter((line) => line.startsWith(prefix))
    .map((line) => line.slice(prefix.length))
  assertEqual(values.length, 1, `${field} signature field count`)
  return values[0]
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
