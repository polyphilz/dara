import { invoke } from '@tauri-apps/api/core'
import type { ImageRecord } from './image-reference.ts'

export function ingestClipboardImage(): Promise<ImageRecord> {
  return invoke<ImageRecord>('ingest_clipboard_image')
}

export async function ingestImageFile(file: File): Promise<ImageRecord> {
  return ingestImageBytes(await file.arrayBuffer())
}

export function ingestImageBytes(
  bytes: ArrayBuffer | Uint8Array,
): Promise<ImageRecord> {
  return invoke<ImageRecord>('ingest_image_bytes', bytes)
}
