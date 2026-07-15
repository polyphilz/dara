import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import 'katex/dist/katex.min.css'
import './styles/base.css'
import './markdown/markdown-renderer.css'
import './markdown/rich-text-editor.css'
import './windows/shared/basic-card-form.css'
import './windows/main/main-window.css'
import { MainWindow } from './windows/main/MainWindow.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MainWindow />
  </StrictMode>,
)
