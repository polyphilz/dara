const MAX_UUID_V7_TIMESTAMP = 0xffff_ffff_ffff

export type FillRandomBytes = (
  bytes: Uint8Array<ArrayBuffer>,
) => Uint8Array<ArrayBuffer>

export function createUuidV7(
  now: number = Date.now(),
  fillRandomBytes: FillRandomBytes = defaultRandomBytes,
): string {
  if (
    !Number.isSafeInteger(now) ||
    now < 0 ||
    now > MAX_UUID_V7_TIMESTAMP
  ) {
    throw new RangeError('UUIDv7 timestamp must be a 48-bit non-negative integer')
  }

  const bytes = new Uint8Array(new ArrayBuffer(16))
  let timestamp = now
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = timestamp % 256
    timestamp = Math.floor(timestamp / 256)
  }

  const random = fillRandomBytes(new Uint8Array(new ArrayBuffer(10)))
  if (random.length !== 10) {
    throw new RangeError('UUIDv7 random source must return exactly 10 bytes')
  }
  bytes[6] = 0x70 | (random[0]! & 0x0f)
  bytes[7] = random[1]!
  bytes[8] = 0x80 | (random[2]! & 0x3f)
  bytes.set(random.subarray(3), 9)

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0'))
  return `${hex.slice(0, 4).join('')}-${hex.slice(4, 6).join('')}-${hex.slice(6, 8).join('')}-${hex.slice(8, 10).join('')}-${hex.slice(10).join('')}`
}

function defaultRandomBytes(
  bytes: Uint8Array<ArrayBuffer>,
): Uint8Array<ArrayBuffer> {
  return globalThis.crypto.getRandomValues(bytes)
}
