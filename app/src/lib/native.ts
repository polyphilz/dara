import { invoke } from '@tauri-apps/api/core'

export interface SpikeStatus {
  panel_ready: boolean
  quick_add_shortcut: string
  review_shortcut: string
  shortcut_errors: string[]
}

export const native = {
  dismissQuickAdd: () => invoke<void>('dismiss_quick_add'),
  getSpikeStatus: () => invoke<SpikeStatus>('get_spike_status'),
  saveSpikeCard: (front: string, back: string) =>
    invoke<void>('save_spike_card', { front, back }),
  showMain: () => invoke<void>('show_main'),
  showQuickAdd: () => invoke<void>('show_quick_add'),
}
