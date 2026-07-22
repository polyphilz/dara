import type { TauriCapabilities } from '@wdio/tauri-service'

const dataDirectory = process.env.DARA_E2E_DATA_DIR
if (!dataDirectory) {
  throw new Error('DARA_E2E_DATA_DIR must be set by the isolated native test runner')
}

const capability: TauriCapabilities = {
  browserName: 'tauri',
  'tauri:options': {
    application: './src-tauri/target/debug/dara',
  },
}

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./tests/native/**/*.spec.ts'],
  maxInstances: 1,
  capabilities: [capability],
  services: [
    [
      '@wdio/tauri-service',
      {
        appBinaryPath: './src-tauri/target/debug/dara',
        captureBackendLogs: true,
        captureFrontendLogs: true,
        clearMocks: false,
        driverProvider: 'embedded',
        env: {
          DARA_DATA_DIR: dataDirectory,
          RUST_LOG: 'info',
        },
        startTimeout: 60_000,
        resetMocks: false,
        restoreMocks: false,
      },
    ],
  ],
  logLevel: 'info',
  bail: 0,
  waitforTimeout: 10_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 0,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    timeout: 60_000,
    ui: 'bdd',
  },
}
