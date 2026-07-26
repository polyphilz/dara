import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createHashHistory } from '@tanstack/react-router'
import '@fontsource-variable/reddit-mono/wght.css'
import 'katex/dist/katex.min.css'
import 'react-activity-calendar/tooltips.css'
import './styles/base.css'
import './markdown/markdown-renderer.css'
import './markdown/rich-text-editor.css'
import './occlusion/occlusion.css'
import './windows/shared/basic-card-form.css'
import './windows/main/main-window.css'
import { MainWindow } from './windows/main/MainWindow.tsx'
import { installAppZoom } from './zoom/app-zoom.ts'
import { installAppAppearance } from './settings/index.ts'

const mainWindowHistory = createHashHistory()

void installAppZoom().catch((error: unknown) => {
  console.error('Could not initialize app zoom', error)
})
void installAppAppearance().catch((error: unknown) => {
  console.error('Could not initialize app appearance', error)
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MainWindow history={mainWindowHistory} />
  </StrictMode>,
)
