import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
import type { DiagnosticsSnapshot } from './contracts.ts'

export interface DiagnosticsGateway {
  loadDiagnostics(): Promise<DiagnosticsSnapshot>
}

export const tauriDiagnosticsGateway: DiagnosticsGateway = {
  loadDiagnostics: () =>
    invoke<DiagnosticsSnapshot>(DaraIpcCommand.LoadDiagnostics),
}
