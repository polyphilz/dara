import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/reddit-mono/wght.css'
import 'katex/dist/katex.min.css'
import './styles/base.css'
import './markdown/rich-text-editor.css'
import './windows/shared/basic-card-form.css'
import './windows/quick-add/quick-add-window.css'
import { QuickAddWindow } from './windows/quick-add/QuickAddWindow.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QuickAddWindow />
  </StrictMode>,
)
