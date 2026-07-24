import { mkdir, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'

const runCount = 2_000
const seedSource = process.env.GITHUB_RUN_ID ?? String(Date.now())
const baseSeed = [...seedSource].reduce(
  (value, character) => (value * 31 + character.charCodeAt(0)) | 0,
  17,
)
const seeds = [baseSeed, baseSeed + 1, baseSeed + 2]
const reportDirectory = path.resolve('test-results/properties')
await mkdir(reportDirectory, { recursive: true })

const report = {
  runCount,
  seedSource,
  seeds,
  startedAt: new Date().toISOString(),
}
await writeReport(report)

for (const seed of seeds) {
  console.info(`property stress seed=${seed} runs=${runCount}`)
  const result = spawnSync(
    'pnpm',
    ['exec', 'vitest', 'run', 'tests/ui/properties'],
    {
      env: {
        ...process.env,
        DARA_PROPERTY_RUNS: String(runCount),
        DARA_PROPERTY_SEED: String(seed),
      },
      stdio: 'inherit',
    },
  )
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

await writeReport({ ...report, completedAt: new Date().toISOString() })

function writeReport(value) {
  return writeFile(
    path.join(reportDirectory, 'seeds.json'),
    `${JSON.stringify(value, null, 2)}\n`,
  )
}
