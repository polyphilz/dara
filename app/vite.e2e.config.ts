import { defineConfig, mergeConfig } from 'vite'
import productionConfig from './vite.config.ts'

const e2eBootstrap = '/tests/native/e2e-bootstrap.ts'

export default mergeConfig(
  productionConfig,
  defineConfig({
    plugins: [
      {
        name: 'dara-e2e-bootstrap',
        transformIndexHtml: {
          order: 'pre',
          handler: () => [
            {
              attrs: { src: e2eBootstrap, type: 'module' },
              injectTo: 'head-prepend',
              tag: 'script',
            },
          ],
        },
      },
    ],
  }),
)
