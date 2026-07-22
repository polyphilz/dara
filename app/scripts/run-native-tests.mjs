import { randomUUID } from 'node:crypto'
import { mkdir, realpath, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'

const appRoot = process.cwd()
const e2eRoot = path.resolve(appRoot, '.data/e2e')
await mkdir(e2eRoot, { recursive: true })
const resolvedRoot = await realpath(e2eRoot)
const runDirectory = path.join(resolvedRoot, `run-${Date.now()}-${randomUUID()}`)
if (path.dirname(runDirectory) !== resolvedRoot) {
  throw new Error(`Refusing native test directory outside ${resolvedRoot}`)
}
await mkdir(runDirectory)
const reportDirectory = path.resolve(appRoot, 'test-results/native')
await mkdir(reportDirectory, { recursive: true })
await writeFile(
  path.join(reportDirectory, 'run.json'),
  `${JSON.stringify({
    dataDirectory: runDirectory,
    startedAt: new Date().toISOString(),
  }, null, 2)}\n`,
)

const environment = {
  ...process.env,
  DARA_DATA_DIR: runDirectory,
  DARA_E2E_DATA_DIR: runDirectory,
}

run('pnpm', [
  'exec',
  'tauri',
  'build',
  '--debug',
  '--no-bundle',
  '--features',
  'e2e',
  '--config',
  'src-tauri/tauri.e2e.conf.json',
])
run('pnpm', ['exec', 'wdio', 'run', 'wdio.conf.ts'])

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: appRoot,
    env: environment,
    stdio: 'inherit',
  })
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}
