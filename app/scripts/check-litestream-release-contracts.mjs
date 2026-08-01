import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { readFileSync, statSync } from 'node:fs'

import {
  DistributionSidecarKey,
  readDistributionSigningPolicy,
} from './distribution-signing.mjs'

const pinPath = 'src-tauri/resources/sidecars/litestream-v1.json'
const noticePath = 'src-tauri/resources/sidecars/litestream-NOTICE'
const baseConfigPath = 'src-tauri/tauri.conf.json'
const releaseConfigPath = 'src-tauri/tauri.release.conf.json'
const mainCapabilityPath = 'src-tauri/capabilities/main.json'
const updaterCapabilityPath = 'src-tauri/capabilities/main-updater.json'
const packagePath = 'package.json'
const rustContractPath = 'src-tauri/src/backup/litestream.rs'
const distributionBuildPath = 'scripts/build-notarized-distribution.mjs'
const releaseArtifactPath = 'scripts/create-release-artifacts.mjs'
const draftReleasePath = 'scripts/publish-draft-release.mjs'
const updaterSigningPath = 'scripts/updater-signing.mjs'
const canaryWorkflowPath = '../.github/workflows/litestream-r2-canary.yml'

const pin = readJson(pinPath)
const baseConfig = readJson(baseConfigPath)
const releaseConfig = readJson(releaseConfigPath)
const mainCapability = readJson(mainCapabilityPath)
const updaterCapability = readJson(updaterCapabilityPath)
const packageJson = readJson(packagePath)
const rustContract = readFileSync(rustContractPath, 'utf8')
const distributionBuild = readFileSync(distributionBuildPath, 'utf8')
const releaseArtifacts = readFileSync(releaseArtifactPath, 'utf8')
const draftRelease = readFileSync(draftReleasePath, 'utf8')
const updaterSigning = readFileSync(updaterSigningPath, 'utf8')
const distributionSigning = readDistributionSigningPolicy()
const litestreamSigningPolicy =
  distributionSigning.sidecars[DistributionSidecarKey.Litestream]
const canaryWorkflow = readFileSync(canaryWorkflowPath, 'utf8')
const notice = readFileSync(noticePath, 'utf8')

assertEqual(pin.manifestVersion, 1, 'Litestream pin format')
assertEqual(pin.component, 'litestream', 'Litestream component')
assertEqual(pin.upstream.releaseTag, 'v0.5.15', 'Litestream release')
assertEqual(pin.target.operatingSystem, 'macos', 'Litestream target OS')
assertEqual(pin.target.architecture, 'arm64', 'Litestream target architecture')
assertEqual(pin.binary.bundlePath, 'bin/litestream', 'Litestream bundle path')
assertEqual(pin.binary.versionOutput, '0.5.15', 'Litestream version output')
assertEqual(
  pin.verification.requiredL0Retention,
  '720h',
  'exact-TXID retention',
)
assert(
  updaterSigning.includes('verifyUpdaterArchiveSignature') &&
    updaterSigning.includes('verifyUpdaterSigningCredentials'),
  'release artifacts do not verify the updater signature',
)
assert(
  packageJson.scripts['release:build:app'].includes(
    'release:stage-provenance',
  ) &&
    distributionBuild.includes('release:stage-provenance') &&
    releaseArtifacts.includes('readPackagedSourceProvenance') &&
    draftRelease.includes('manifest.source?.commit') &&
    draftRelease.includes('manifest.source?.dirty === false'),
  'release artifacts are not bound to a clean source commit',
)
for (const [name, value] of Object.entries(pin.verification)) {
  if (typeof value === 'boolean') {
    assert(value, `Litestream protocol verification is not pinned green: ${name}`)
  }
}
assert(/^[a-f0-9]{64}$/.test(pin.upstream.asset.sha256), 'invalid archive SHA-256')
assert(/^[a-f0-9]{64}$/.test(pin.binary.sha256), 'invalid binary SHA-256')
assert(pin.upstream.asset.size > 0, 'invalid archive byte length')
assert(pin.binary.size > 0, 'invalid binary byte length')
assert(
  pin.upstream.asset.url.includes(`/download/${pin.upstream.releaseTag}/`),
  'Litestream asset URL is not pinned to its release tag',
)

const resourceMappings = releaseConfig.bundle.resources
assertEqual(
  resourceMappings['resources/release/bin/litestream'],
  pin.resourceDestinations.binary,
  'Tauri Litestream binary mapping',
)
assertEqual(
  resourceMappings['resources/release/litestream.json'],
  pin.resourceDestinations.releaseManifest,
  'Tauri Litestream manifest mapping',
)
assertEqual(
  resourceMappings['resources/release/licenses/litestream-LICENSE'],
  pin.resourceDestinations.license,
  'Tauri Litestream license mapping',
)
assertEqual(
  resourceMappings['resources/release/licenses/litestream-NOTICE'],
  pin.resourceDestinations.notice,
  'Tauri Litestream notice mapping',
)
assertEqual(
  releaseConfig.bundle.macOS.minimumSystemVersion,
  pin.target.minimumSystemVersion,
  'packaged minimum macOS version',
)
assertEqual(
  litestreamSigningPolicy.bundlePath,
  pin.binary.bundlePath,
  'Developer ID Litestream bundle path',
)
assertEqual(
  litestreamSigningPolicy.component,
  pin.component,
  'Developer ID Litestream component',
)
assertEqual(
  litestreamSigningPolicy.identifier,
  'com.silo77.dara.sidecar.litestream',
  'Developer ID Litestream requirement identifier',
)

assert(
  packageJson.scripts['release:stage-sidecars'].includes(
    'release:stage-litestream',
  ),
  'release staging does not include Litestream',
)
for (const command of [
  'release:verify-contracts',
  'release:stage-sidecars',
  'release:verify-resources',
  'tauri build',
  'release:verify-app',
]) {
  assert(
    packageJson.scripts['release:build:app'].includes(command),
    `release build omits ${command}`,
  )
}
assert(
  !packageJson.scripts['release:build:app'].includes(
    'VITE_DARA_UPDATER_ENABLED',
  ),
  'ad-hoc release application unexpectedly enables the updater frontend',
)
assert(
  distributionBuild.includes('release:verify-updater-signing') &&
    distributionBuild.includes("VITE_DARA_UPDATER_ENABLED: 'true'") &&
    distributionBuild.includes("'main-updater'"),
  'notarized distribution build does not enable the updater frontend',
)
for (const permission of [
  'process:allow-restart',
  'updater:allow-check',
  'updater:allow-download-and-install',
]) {
  assert(
    !mainCapability.permissions.includes(permission),
    `ordinary main-window capability grants production updater permission: ${permission}`,
  )
  assert(
    updaterCapability.permissions.includes(permission),
    `production updater capability omits ${permission}`,
  )
}
assert(
  !baseConfig.app.security.capabilities.includes('main-updater'),
  'ordinary builds unexpectedly grant the production updater capability',
)
for (const contract of [
  "COPYFILE_DISABLE: '1'",
  "'--no-mac-metadata'",
  'verifyExtractedUpdaterArchive',
  "'/usr/bin/codesign'",
  "'/usr/sbin/spctl'",
]) {
  assert(
    releaseArtifacts.includes(contract),
    `updater archive validation omits ${contract}`,
  )
}
assert(
  updaterSigning.includes('scripts/updater-signature-verifier/Cargo.toml') &&
    !updaterSigning.includes("'src-tauri/Cargo.toml'"),
  'updater signature verification is not isolated from the application crate',
)
assert(
  typeof baseConfig.plugins?.updater?.pubkey === 'string' &&
    baseConfig.plugins.updater.pubkey.length > 0,
  'Tauri updater public key is missing',
)
assertEqual(
  baseConfig.plugins.updater.endpoints,
  ['https://github.com/polyphilz/dara/releases/latest/download/latest.json'],
  'Tauri updater endpoint',
)
assert(
  rustContract.includes(
    'include_str!("../../resources/sidecars/litestream-v1.json")',
  ),
  'Rust does not embed the pinned Litestream manifest',
)
for (const contract of [
  'l0-retention: 720h',
  'auto-recover: false',
  'verify-compaction: true',
  'DARA_LITESTREAM_R2_ACCESS_KEY_ID',
  'DARA_LITESTREAM_R2_SECRET_ACCESS_KEY',
  'EMBEDDED_DISTRIBUTION_SIGNING_POLICY',
  'verify_distribution_signature',
]) {
  assert(rustContract.includes(contract), `Rust Litestream contract omits ${contract}`)
}
assert(
  notice.includes('Apache License 2.0'),
  'Litestream notice does not identify its license',
)
assert(statSync(noticePath).isFile(), 'Litestream notice is not a regular file')

for (const forbiddenTrigger of ['pull_request:', 'pull_request_target:']) {
  assert(
    !canaryWorkflow.includes(forbiddenTrigger),
    `real-R2 canary exposes a forbidden trigger: ${forbiddenTrigger}`,
  )
}
for (const contract of [
  'workflow_dispatch:',
  'schedule:',
  "- cron: '29 8 * * 1'",
  'environment: r2-canary',
  'secrets.DARA_LITESTREAM_R2_ACCOUNT_ID',
  'secrets.DARA_LITESTREAM_R2_BUCKET',
  'secrets.DARA_LITESTREAM_R2_ACCESS_KEY_ID',
  'secrets.DARA_LITESTREAM_R2_SECRET_ACCESS_KEY',
  'app/.data/r2-canary/*/canary-report-v1.json',
]) {
  assert(canaryWorkflow.includes(contract), `real-R2 canary omits ${contract}`)
}
assert(
  !canaryWorkflow.includes('path: app/.data/r2-canary/'),
  'real-R2 canary uploads its full local restore instead of bounded reports',
)

const tracked = execFileSync('git', ['ls-files', '--full-name', '-z'], {
  encoding: 'utf8',
})
  .split('\0')
  .filter(Boolean)
for (const path of tracked) {
  const normalized = path.toLowerCase()
  assert(
    !normalized.endsWith('/.env.local') && !normalized.endsWith('.env.local'),
    `a local secret environment file is tracked: ${path}`,
  )
  assert(
    !normalized.endsWith('/.env.notarization') &&
      !normalized.endsWith('.env.notarization'),
    `a local Apple notarization environment file is tracked: ${path}`,
  )
  assert(
    !normalized.endsWith('/.env.updater') &&
      !normalized.endsWith('.env.updater'),
    `a local updater signing environment file is tracked: ${path}`,
  )
  assert(
    !/\.(?:key|mobileprovision|p8|p12|pem|pfx|certsigningrequest)$/u.test(normalized),
    `a private signing credential is tracked: ${path}`,
  )
  assert(
    !normalized.startsWith('app/src-tauri/resources/release/'),
    `generated release staging is tracked: ${path}`,
  )
  assert(
    !normalized.includes('litestream.yml') &&
      !normalized.includes('litestream.yaml'),
    `generated Litestream configuration is tracked: ${path}`,
  )
}

console.info(
  `Litestream release contracts passed: ${pin.upstream.releaseTag}, ${pin.target.architecture}, ${pin.binary.sha256}, ${sha256(notice)} notice.`,
)

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
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
