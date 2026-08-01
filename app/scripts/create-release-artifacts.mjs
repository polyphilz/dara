import { createHash } from 'node:crypto'
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { basename, dirname, relative, resolve, sep } from 'node:path'

import { assert, assertEqual, run } from './distribution-signing.mjs'
import { readReleaseVersion } from './release-version.mjs'

const UpdaterEnvironmentVariable = Object.freeze({
  PrivateKeyPassword: 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  PrivateKeyPath: 'TAURI_SIGNING_PRIVATE_KEY_PATH',
})
const updaterEnvironmentPath = resolve('.env.updater')
const repositoryRoot = resolve('..')
const releaseRoot = resolve('src-tauri/target/release/bundle/release')

if (existsSync(updaterEnvironmentPath)) {
  process.loadEnvFile(updaterEnvironmentPath)
}

const scriptArguments = process.argv.slice(2)
if (scriptArguments[0] === '--') {
  scriptArguments.shift()
}
assert(
  scriptArguments.length === 0 || scriptArguments.length === 2,
  'usage: node scripts/create-release-artifacts.mjs [<Dara.app> <Dara.dmg>]',
)

const version = readReleaseVersion()
const [applicationArgument, diskImageArgument] = scriptArguments
const applicationPath = applicationArgument
  ? resolve(applicationArgument)
  : resolve('src-tauri/target/release/bundle/macos/Dara.app')
const diskImagePath = diskImageArgument
  ? resolve(diskImageArgument)
  : resolve(`src-tauri/target/release/bundle/dmg/Dara_${version}_aarch64.dmg`)
const updaterCredentials = takeUpdaterEnvironment()

preflight(applicationPath, diskImagePath, updaterCredentials)
run('node', [
  'scripts/check-notarized-distribution.mjs',
  applicationPath,
  diskImagePath,
], { stdio: 'inherit' })
rmSync(releaseRoot, { recursive: true, force: true })
mkdirSync(releaseRoot, { recursive: true })

const diskImageName = `Dara_${version}_aarch64.dmg`
const updaterArchiveName = `Dara_${version}_aarch64.app.tar.gz`
const diskImageReleasePath = resolve(releaseRoot, diskImageName)
const updaterArchivePath = resolve(releaseRoot, updaterArchiveName)
const updaterSignaturePath = `${updaterArchivePath}.sig`
const manifestPath = resolve(releaseRoot, 'latest.json')
const checksumsPath = resolve(releaseRoot, 'SHA256SUMS')

copyFileSync(diskImagePath, diskImageReleasePath)
createUpdaterArchive(applicationPath, updaterArchivePath)
signUpdaterArchive(updaterArchivePath, updaterCredentials)

const signature = readFileSync(updaterSignaturePath, 'utf8').trim()
assert(signature.length > 0, 'the updater signature is empty')
writeFileSync(
  manifestPath,
  `${JSON.stringify(
    {
      version,
      notes: `See the Dara v${version} release notes on GitHub.`,
      pub_date: new Date().toISOString(),
      platforms: {
        'darwin-aarch64': {
          signature,
          url:
            `https://github.com/polyphilz/dara/releases/download/` +
            `v${version}/${updaterArchiveName}`,
        },
      },
    },
    null,
    2,
  )}\n`,
)

const releaseFiles = [
  diskImageReleasePath,
  updaterArchivePath,
  updaterSignaturePath,
  manifestPath,
]
writeFileSync(
  checksumsPath,
  `${releaseFiles
    .map((path) => `${sha256File(path)}  ${basename(path)}`)
    .join('\n')}\n`,
)

console.info(`Release artifacts passed: ${releaseRoot}`)

function takeUpdaterEnvironment() {
  const values = Object.fromEntries(
    Object.values(UpdaterEnvironmentVariable).map((name) => [
      name,
      process.env[name]?.trim(),
    ]),
  )
  for (const name of Object.values(UpdaterEnvironmentVariable)) {
    delete process.env[name]
  }
  const value = (name) => {
    const result = values[name]
    assert(
      result,
      `${name} is required; copy .env.updater.example to .env.updater`,
    )
    return result
  }
  return {
    privateKeyPassword: value(
      UpdaterEnvironmentVariable.PrivateKeyPassword,
    ),
    privateKeyPath: resolve(value(UpdaterEnvironmentVariable.PrivateKeyPath)),
  }
}

function preflight(applicationPath_, diskImagePath_, credentials) {
  assert(process.platform === 'darwin', 'release artifacts require macOS')
  assert(process.arch === 'arm64', 'release artifacts require arm64 macOS')
  assert(existsSync(applicationPath_), `application is missing: ${applicationPath_}`)
  assert(existsSync(diskImagePath_), `disk image is missing: ${diskImagePath_}`)
  assert(
    existsSync(credentials.privateKeyPath),
    `updater private key is missing: ${credentials.privateKeyPath}`,
  )
  assert(
    statSync(credentials.privateKeyPath).isFile(),
    'updater private key is not a regular file',
  )
  const privateKeyPath = realpathSync(credentials.privateKeyPath)
  const relativeKeyPath = relative(realpathSync(repositoryRoot), privateKeyPath)
  assert(
    relativeKeyPath === '..' || relativeKeyPath.startsWith(`..${sep}`),
    'updater private key must be stored outside the Dara repository',
  )
  if ((statSync(privateKeyPath).mode & 0o077) !== 0) {
    chmodSync(privateKeyPath, 0o600)
  }
  assertEqual(
    run('/usr/libexec/PlistBuddy', [
      '-c',
      'Print :CFBundleShortVersionString',
      resolve(applicationPath_, 'Contents/Info.plist'),
    ], { capture: true }),
    version,
    'packaged application version',
  )
}

function createUpdaterArchive(applicationPath_, archivePath) {
  mkdirSync(dirname(archivePath), { recursive: true })
  rmSync(archivePath, { force: true })
  rmSync(`${archivePath}.sig`, { force: true })
  run('tar', [
    '-czf',
    archivePath,
    '-C',
    dirname(applicationPath_),
    basename(applicationPath_),
  ], { stdio: 'inherit' })
  const entries = run('tar', ['-tzf', archivePath], { capture: true })
    .split(/\r?\n/u)
    .filter(Boolean)
  const root = `${basename(applicationPath_)}/`
  assert(entries.length > 1, 'the updater archive is empty')
  assert(
    entries.every((entry) => entry === basename(applicationPath_) || entry.startsWith(root)),
    'the updater archive contains a path outside Dara.app',
  )
}

function signUpdaterArchive(archivePath, credentials) {
  run('pnpm', [
    'exec',
    'tauri',
    'signer',
    'sign',
    '--private-key-path',
    credentials.privateKeyPath,
    archivePath,
  ], {
    env: {
      ...process.env,
      [UpdaterEnvironmentVariable.PrivateKeyPassword]:
        credentials.privateKeyPassword,
    },
    stdio: 'inherit',
  })
  assert(
    existsSync(`${archivePath}.sig`),
    `updater signature was not created: ${archivePath}.sig`,
  )
}

function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}
