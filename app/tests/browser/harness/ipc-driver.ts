import type { Event } from '@tauri-apps/api/event'
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { emit, listen } from '@tauri-apps/api/event'
import { DaraEvent, type DaraEvent as DaraEventType } from '../../../src/lib/tauri-contracts.ts'
import { FakeDaraBackend, type FakeBackendSnapshot } from './fake-dara-backend.ts'
import {
  BrowserHarnessSurface,
  type BrowserScenario,
} from './scenarios.ts'

export interface RecordedDaraEvent {
  event: DaraEventType
  payload: unknown
}

export interface DaraBrowserTestApi {
  emit(event: DaraEventType, payload?: unknown): Promise<void>
  events(): RecordedDaraEvent[]
  snapshot(): FakeBackendSnapshot
}

export async function installIpcDriver(
  scenario: BrowserScenario,
  surface: BrowserHarnessSurface,
): Promise<{ api: DaraBrowserTestApi; dispose: () => void }> {
  const backend = new FakeDaraBackend(scenario)
  const events: RecordedDaraEvent[] = []
  mockIPC(
    (command, payload) => backend.invoke(command, payload),
    { shouldMockEvents: true },
  )
  const currentWindow = surface === BrowserHarnessSurface.QuickAdd
    ? 'quick-add'
    : 'main'
  const otherWindow = currentWindow === 'main' ? 'quick-add' : 'main'
  mockWindows(currentWindow, otherWindow)
  const unlistenCardCreated = await listen(DaraEvent.CardCreated, (event: Event<unknown>) => {
    events.push({ event: DaraEvent.CardCreated, payload: structuredClone(event.payload) })
  })
  return {
    api: {
      emit: (event, payload) => emit(event, payload),
      events: () => structuredClone(events),
      snapshot: () => backend.snapshot(),
    },
    dispose: () => {
      unlistenCardCreated()
      clearMocks()
    },
  }
}
