import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/reddit-mono/wght.css'
import 'katex/dist/katex.min.css'
import '../../../src/styles/base.css'
import '../../../src/markdown/rich-text-editor.css'
import '../../../src/occlusion/occlusion.css'
import '../../../src/windows/shared/basic-card-form.css'
import '../../../src/windows/quick-add/quick-add-window.css'
import { Appearance } from '../../../src/settings/types.ts'
import { installIpcDriver, type DaraBrowserTestApi } from './ipc-driver.ts'
import {
  BrowserHarnessSurface,
  parseBrowserHarnessSurface,
  parseBrowserScenario,
} from './scenarios.ts'

declare global {
  interface Window {
    __DARA_BROWSER_TEST__: DaraBrowserTestApi
  }
}

const parameters = new URLSearchParams(window.location.search)
const scenario = parseBrowserScenario(parameters.get('scenario'))
const surface = parseBrowserHarnessSurface(parameters.get('surface'))
const appearance = parameters.get('appearance')
if (appearance === Appearance.Dark || appearance === Appearance.Light) {
  document.documentElement.dataset.appearance = appearance
}
const driver = await installIpcDriver(scenario, surface)
window.__DARA_BROWSER_TEST__ = driver.api
window.addEventListener('pagehide', driver.dispose, { once: true })

const component =
  surface === BrowserHarnessSurface.VisualCatalog
    ? (await import('./visual-catalog.tsx')).VisualCatalog
    : surface === BrowserHarnessSurface.Main
      ? (await import('./main-surface.tsx')).MainWindow
      : surface === BrowserHarnessSurface.Recovery
        ? (await import('./recovery-surface.tsx')).RecoveryWindow
        : (await import('../../../src/windows/quick-add/QuickAddWindow.tsx')).QuickAddWindow
const Component = component

createRoot(document.getElementById('root')!).render(<StrictMode><Component /></StrictMode>)
