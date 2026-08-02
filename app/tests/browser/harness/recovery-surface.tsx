import { RecoveryWindow as RecoveryWindowComponent } from '../../../src/recovery/RecoveryWindow.tsx'
import type { FreshInstallRecoveryGateway } from '../../../src/recovery/index.ts'

/*
 * A deterministic first-launch recovery surface. Discovery and restore never
 * settle, so the welcome copy and choices stay fixed for text-resilience and
 * appearance checks without depending on time or backend state.
 */
const recoveryGateway: FreshInstallRecoveryGateway = {
  loadLaunchContext: () => new Promise(() => undefined),
  startFresh: () => new Promise(() => undefined),
  discover: () => new Promise(() => undefined),
  restore: () => new Promise(() => undefined),
}

export function RecoveryWindow() {
  return <RecoveryWindowComponent gateway={recoveryGateway} />
}
