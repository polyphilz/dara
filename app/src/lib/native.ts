import { invoke } from '@tauri-apps/api/core'

export const native = {
  dismissQuickAdd: () => invoke<void>('dismiss_quick_add'),
  openExternalUrl: (url: string) =>
    invoke<void>('open_external_url', { url }),
  setQuickAddFileDialogOpen: (open: boolean) =>
    invoke<void>('set_quick_add_file_dialog_open', { open }),
  showMain: () => invoke<void>('show_main'),
  showQuickAdd: () => invoke<void>('show_quick_add'),
}
