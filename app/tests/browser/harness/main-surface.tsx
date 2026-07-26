import { createHashHistory } from '@tanstack/react-router'
import 'react-activity-calendar/tooltips.css'
import '../../../src/markdown/markdown-renderer.css'
import '../../../src/windows/main/main-window.css'
import { MainWindow as DaraMainWindow } from '../../../src/windows/main/MainWindow.tsx'

const mainWindowHistory = createHashHistory()

export function MainWindow() {
  return <DaraMainWindow history={mainWindowHistory} />
}
