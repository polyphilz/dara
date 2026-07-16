import { invoke } from '@tauri-apps/api/core'
import type { ImageRecord } from './image-reference.ts'

const MEDIA_LEASE_ID_BYTE_LENGTH = 36

const MediaCommand = {
  IngestClipboardImage: 'ingest_clipboard_image',
  IngestImageBytes: 'ingest_image_bytes',
  Maintain: 'maintain_media',
  RenewLease: 'renew_media_lease',
} as const

export function ingestClipboardImage(leaseId: string): Promise<ImageRecord> {
  return invoke<ImageRecord>(MediaCommand.IngestClipboardImage, { leaseId })
}

export async function ingestImageFile(
  file: File,
  leaseId: string,
): Promise<ImageRecord> {
  return ingestImageBytes(await file.arrayBuffer(), leaseId)
}

export function ingestImageBytes(
  bytes: ArrayBuffer | Uint8Array,
  leaseId: string,
): Promise<ImageRecord> {
  const leaseBytes = new TextEncoder().encode(leaseId)
  if (leaseBytes.byteLength !== MEDIA_LEASE_ID_BYTE_LENGTH) {
    return Promise.reject(new Error('Media lease ID must be a canonical UUID.'))
  }
  const imageBytes = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
  const payload = new Uint8Array(leaseBytes.byteLength + imageBytes.byteLength)
  payload.set(leaseBytes)
  payload.set(imageBytes, leaseBytes.byteLength)
  return invoke<ImageRecord>(MediaCommand.IngestImageBytes, payload)
}

export function renewMediaLease(leaseId: string): Promise<number> {
  return invoke<number>(MediaCommand.RenewLease, { leaseId })
}

export interface MediaMaintenanceReport {
  inspectedAt: number
  integrity: {
    orphanedImageIds: string[]
    extraBlobSha256: string[]
    missingReferencedBlobImageIds: string[]
    extraBlobBytes: number
  }
  cleanup: {
    retiredImageCount: number
    deletedBlobCount: number
    reclaimedBytes: number
  }
}

export function maintainMedia(): Promise<MediaMaintenanceReport> {
  return invoke<MediaMaintenanceReport>(MediaCommand.Maintain)
}
