import { invoke } from '@tauri-apps/api/core'
import { DaraIpcCommand } from '../lib/tauri-contracts.ts'
import type { ImageRecord } from './image-reference.ts'

const MEDIA_LEASE_ID_BYTE_LENGTH = 36

export function ingestClipboardImage(leaseId: string): Promise<ImageRecord> {
  return invoke<ImageRecord>(DaraIpcCommand.IngestClipboardImage, { leaseId })
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
  return invoke<ImageRecord>(DaraIpcCommand.IngestImageBytes, payload)
}

export function renewMediaLease(leaseId: string): Promise<number> {
  return invoke<number>(DaraIpcCommand.RenewMediaLease, { leaseId })
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
  return invoke<MediaMaintenanceReport>(DaraIpcCommand.MaintainMedia)
}
