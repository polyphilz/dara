import { invoke } from '@tauri-apps/api/core'
import type { ImageRecord } from './image-reference.ts'

export function ingestClipboardImage(): Promise<ImageRecord> {
  return invoke<ImageRecord>('ingest_clipboard_image')
}
