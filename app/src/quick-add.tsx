import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/base.css'
import './windows/quick-add/quick-add-window.css'
import { QuickAddWindow } from './windows/quick-add/QuickAddWindow.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QuickAddWindow />
  </StrictMode>,
)
