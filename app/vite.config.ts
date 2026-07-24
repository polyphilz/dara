import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { bundleReportPlugin } from './build/bundle-report.ts'

// https://vite.dev/config/
export default defineConfig({
  clearScreen: false,
  plugins: [react(), bundleReportPlugin()],
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  optimizeDeps: {
    entries: [
      'index.html',
      'quick-add.html',
    ],
  },
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'oxc',
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    rolldownOptions: {
      input: {
        main: 'index.html',
        quickAdd: 'quick-add.html',
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})
