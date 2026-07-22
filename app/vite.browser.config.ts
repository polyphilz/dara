import { defineConfig, mergeConfig } from 'vite'
import productionConfig from './vite.config.ts'

export default mergeConfig(
  productionConfig,
  defineConfig({
    optimizeDeps: {
      entries: ['tests/browser/harness/index.html'],
    },
  }),
)
