import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { readFileSync, statSync } from 'node:fs'

const pinPath = 'src-tauri/resources/sidecars/litestream-v1.json'
const noticePath = 'src-tauri/resources/sidecars/litestream-NOTICE'
const releaseConfigPath = 'src-tauri/tauri.release.conf.json'
const packagePath = 'package.json'
const rustContractPath = 'src-tauri/src/backup/litestream.rs'
const canaryWorkflowPath = '../.github/workflows/litestream-r2-canary.yml'

const pin = readJson(pinPath)
const releaseConfig = readJson(releaseConfigPath)
const packageJson = readJson(packagePath)
const rustContract = readFileSync(rustContractPath, 'utf8')
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
