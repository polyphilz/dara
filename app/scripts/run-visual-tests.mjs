import { spawnSync } from 'node:child_process'

const VisualTestMode = Object.freeze({
  Compare: 'compare',
  Update: 'update',
})

const VisualProject = Object.freeze({
  Standard: 'webkit-visual',
  Hidpi: 'webkit-hidpi-visual',
})

const CANONICAL_PLATFORM = 'darwin'
const CANONICAL_ARCHITECTURE = 'arm64'
const CANONICAL_UPDATE_ENVIRONMENT_VARIABLE =
  'DARA_CANONICAL_VISUAL_UPDATE'
const ENABLED_ENVIRONMENT_VALUE = '1'
const GITHUB_ACTIONS_CI_VALUE = 'true'

function fail(message) {
  console.error(message)
  return 1
}

function main() {
  const mode = process.argv[2]
  if (!Object.values(VisualTestMode).includes(mode)) {
    return fail(
      `Expected visual test mode to be one of: ${Object.values(VisualTestMode).join(', ')}`,
    )
  }

  if (
    process.platform !== CANONICAL_PLATFORM ||
    process.arch !== CANONICAL_ARCHITECTURE
  ) {
    return fail(
      `Canonical visual tests require macOS ARM64; received ${process.platform} ${process.arch}.`,
    )
  }

  if (
    mode === VisualTestMode.Update &&
    (process.env.CI !== GITHUB_ACTIONS_CI_VALUE ||
      process.env[CANONICAL_UPDATE_ENVIRONMENT_VARIABLE] !==
        ENABLED_ENVIRONMENT_VALUE)
  ) {
    return fail(
      'Canonical baselines can only be updated by the Frontend visual GitHub Actions workflow. Trigger it with update_snapshots enabled, or download the baseline artifact from a failed visual run.',
    )
  }

  const args = [
    'exec',
    'playwright',
    'test',
    `--project=${VisualProject.Standard}`,
    `--project=${VisualProject.Hidpi}`,
  ]
  if (mode === VisualTestMode.Update) {
    args.push('--update-snapshots')
  }

  const result = spawnSync('pnpm', args, { stdio: 'inherit' })
  if (result.error) {
    return fail(`Unable to start Playwright: ${result.error.message}`)
  }
  return result.status ?? 1
}

process.exitCode = main()
