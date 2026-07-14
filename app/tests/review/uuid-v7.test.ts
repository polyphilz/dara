import assert from 'node:assert/strict'
import test from 'node:test'
import { createUuidV7 } from '../../src/review/uuid-v7.ts'

test('creates canonical lowercase UUIDv7 identifiers', () => {
  const id = createUuidV7(0x0198_0c8e_6c00, (bytes) => bytes.fill(0))
  assert.equal(id, '01980c8e-6c00-7000-8000-000000000000')
  assert.equal(id[14], '7')
  assert.match(id[19]!, /[89ab]/)
})

test('rejects timestamps outside the UUIDv7 48-bit field', () => {
  assert.throws(() => createUuidV7(-1), /48-bit/)
  assert.throws(() => createUuidV7(0x1_0000_0000_0000), /48-bit/)
})
