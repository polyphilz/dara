import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

import {
  assert,
  assertEqual,
  readDistributionSigningPolicy,
  run,
  signatureField,
  signatureFields,
} from './distribution-signing.mjs'

const policy = readDistributionSigningPolicy()
const appPath = resolve(
  process.argv[2] ?? 'src-tauri/target/release/bundle/macos/Dara.app',
)
const dmgPath = resolve(requiredArgument(3, 'notarized DMG path'))

assert(existsSync(appPath), `application was not found: ${appPath}`)
assert(existsSync(dmgPath), `disk image was not found: ${dmgPath}`)

run('/usr/bin/codesign', ['--verify', '--strict', '--verbose=2', dmgPath])
const dmgSignature = run('/usr/bin/codesign', [
  '--display',
  '--verbose=4',
  dmgPath,
])
assertEqual(
  signatureField(dmgSignature, 'TeamIdentifier'),
  policy.application.teamIdentifier,
  'DMG signing team',
)
assertEqual(
  signatureFields(dmgSignature, 'Authority')[0],
  policy.application.signingIdentity,
  'DMG leaf signing authority',
)
run('xcrun', ['stapler', 'validate', dmgPath])
run('/usr/sbin/spctl', [
  '--assess',
  '--type',
  'open',
  '--context',
  'context:primary-signature',
  '--verbose=4',
  dmgPath,
])

const mountRoot = mkdtempSync(join(tmpdir(), 'dara-distribution-mount-'))
const mountPoint = join(mountRoot, 'volume')
mkdirSync(mountPoint)
let attached = false
try {
  run('hdiutil', [
    'attach',
    dmgPath,
    '-readonly',
    '-nobrowse',
    '-mountpoint',
    mountPoint,
  ])
  attached = true
  const mountedAppPath = join(mountPoint, 'Dara.app')
  assert(
    existsSync(mountedAppPath),
    'notarized disk image does not contain Dara.app',
  )
  run('node', [
    'scripts/check-packaged-app.mjs',
    mountedAppPath,
    'developer-id',
  ])
  run('xcrun', ['stapler', 'validate', mountedAppPath])
  run('/usr/sbin/spctl', [
    '--assess',
    '--type',
    'execute',
    '--verbose=4',
    mountedAppPath,
  ])
} finally {
  if (attached) {
    run('hdiutil', ['detach', mountPoint])
  }
  rmSync(mountRoot, { recursive: true, force: true })
}

console.info(`Notarization, stapling, Gatekeeper, and DMG contents passed.`)

function requiredArgument(index, label) {
  const value = process.argv[index]
  assert(value, `${label} is required`)
  return value
}
