import { mkdir, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import process from 'node:process'

const repetitions = 3
const startedAt = new Date().toISOString()
await mkdir('test-results/native', { recursive: true })

for (let repetition = 1; repetition <= repetitions; repetition += 1) {
  console.info(`native stress repetition=${repetition}/${repetitions}`)
  const result = spawnSync('pnpm', ['test:native'], { stdio: 'inherit' })
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

await writeFile(
  'test-results/native/stress.json',
  `${JSON.stringify({
    completedAt: new Date().toISOString(),
    repetitions,
    startedAt,
  }, null, 2)}\n`,
)
