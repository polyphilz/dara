import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useMemo, useSyncExternalStore } from 'react'
import { DaraEvent } from '../lib/tauri-contracts.ts'
import {
  tauriSettingsGateway,
  type SettingsSnapshot,
} from '../settings/index.ts'
import { updaterIsEnabled } from './contracts.ts'
import { UpdateController } from './controller.ts'
import { tauriUpdateGateway } from './gateway.ts'
import { UpdateNotification } from './UpdateNotification.tsx'

export function AppUpdater() {
  const tauriEnvironment = isTauri()
  const enabled = updaterIsEnabled(
    tauriEnvironment,
    import.meta.env.VITE_DARA_UPDATER_ENABLED,
  )
  const controller = useMemo(
    () =>
      new UpdateController(tauriUpdateGateway, {
        automaticChecksEnabled: false,
        enabled,
      }),
    [enabled],
  )
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
  )

  useEffect(() => controller.start(), [controller])

  useEffect(() => {
    if (!tauriEnvironment) {
      return
    }
    let disposed = false
    let stopListening: (() => void) | undefined
    const applySettings = (settings: SettingsSnapshot) => {
      controller.setAutomaticChecksEnabled(
        settings.automaticUpdateChecksEnabled,
      )
    }
    void tauriSettingsGateway
      .loadSettings()
      .then((settings) => {
        if (!disposed) {
          applySettings(settings)
        }
      })
      .catch((error: unknown) => {
        console.error('Could not load automatic update settings', error)
      })
    void listen<SettingsSnapshot>(DaraEvent.SettingsChanged, (event) => {
      if (!disposed) {
        applySettings(event.payload)
      }
    }).then((unlisten) => {
      if (disposed) {
        unlisten()
      } else {
        stopListening = unlisten
      }
    })
    return () => {
      disposed = true
      stopListening?.()
    }
  }, [controller, tauriEnvironment])

  useEffect(() => {
    if (!tauriEnvironment) {
      return
    }
    let disposed = false
    const listener = listen(DaraEvent.CheckForUpdates, () => {
      void controller.checkManually()
    }).then((unlisten) => {
      if (disposed) {
        unlisten()
        return null
      }
      return unlisten
    })
    return () => {
      disposed = true
      void listener.then((unlisten) => unlisten?.())
    }
  }, [controller, tauriEnvironment])

  return <UpdateNotification controller={controller} state={state} />
}
