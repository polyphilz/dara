import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from './tauri-contracts.ts'

export const native = {
  dismissQuickAdd: () => invoke<void>(DaraIpcCommand.DismissQuickAdd),
  openExternalUrl: (url: string) =>
    invoke<void>(DaraIpcCommand.OpenExternalUrl, { url }),
  setQuickAddFileDialogOpen: (open: boolean) =>
    invoke<void>(DaraIpcCommand.SetQuickAddFileDialogOpen, { open }),
  showMain: () => invoke<void>(DaraIpcCommand.ShowMain),
  showQuickAdd: () => invoke<void>(DaraIpcCommand.ShowQuickAdd),
}
