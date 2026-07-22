import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import process from 'node:process'

const manifest = 'src-tauri/Cargo.toml'
const productionTree = cargoTree([])
const e2eTree = cargoTree(['--features', 'e2e'])
const productionConfig = readFileSync('src-tauri/tauri.conf.json', 'utf8')

for (const marker of ['tauri-plugin-wdio', 'wdio-webdriver']) {
  if (productionTree.includes(marker)) {
    throw new Error(`Ordinary Cargo graph contains ${marker}`)
  }
  if (productionConfig.includes(marker) || productionConfig.includes('withGlobalTauri')) {
    throw new Error(`Ordinary Tauri config contains E2E marker ${marker}`)
  }
}
for (const required of ['tauri-plugin-wdio v', 'tauri-plugin-wdio-webdriver v']) {
  if (!e2eTree.includes(required)) {
    throw new Error(`E2E Cargo graph is missing ${required}`)
  }
}

console.info('Native bundle safety passed: WDIO plugins are feature-gated out of the ordinary Cargo graph and Tauri config.')

function cargoTree(extraArguments) {
  const result = spawnSync(
    'cargo',
    ['tree', '--locked', '--manifest-path', manifest, '-e', 'normal', ...extraArguments],
    { encoding: 'utf8' },
  )
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.stderr.write(result.stderr)
    process.exit(result.status ?? 1)
  }
  return result.stdout
}
