import {
  fireEvent,
  render,
  waitFor,
} from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { R2Jurisdiction } from '../../../src/backup/index.ts'
import { RecoveryWindow } from '../../../src/recovery/RecoveryWindow.tsx'
import {
  RemoteCheckpointAvailability,
  type FreshInstallRecoveryGateway,
} from '../../../src/recovery/index.ts'

const ACCOUNT_ID = '0123456789abcdef0123456789abcdef'
const ACCESS_KEY_ID = '11111111111111111111111111111111'
const SECRET_ACCESS_KEY =
  '2222222222222222222222222222222222222222222222222222222222222222'

test('offers only start-fresh and restore choices on a fresh installation', () => {
  const gateway = gatewayFixture()
  const { getByRole, getByText, queryByLabelText } = render(
    <RecoveryWindow gateway={gateway} />,
  )

  expect(
    getByText('dara', { selector: '.recovery-brand-wordmark' }),
  ).toBeTruthy()
  expect(getByText('This device does not have any Dara data yet.')).toBeTruthy()
  const startFresh = getByRole('button', { name: /start fresh/i })
  const restore = getByRole('button', { name: /restore from backup/i })
  expect(startFresh.className).toContain('dara-button')
  expect(restore.className).toContain('dara-button')
  expect(queryByLabelText('R2 account ID')).toBeNull()
})

test('validates R2 details locally before trying discovery', () => {
  const gateway = gatewayFixture()
  const { getByRole, getByText } = render(
    <RecoveryWindow gateway={gateway} />,
  )

  fireEvent.click(getByRole('button', { name: /restore from backup/i }))
  fireEvent.click(getByRole('button', { name: 'Find backups' }))

  expect(
    getByText('Enter the 32-character lowercase R2 account ID.'),
  ).toBeTruthy()
  expect(gateway.discover).not.toHaveBeenCalled()
})

test('clears credentials and displays complete remote checkpoints after discovery', async () => {
  const gateway = gatewayFixture()
  gateway.discover.mockResolvedValueOnce({
    checkpoints: [
      {
        checkpointId: '019f547b-6200-7000-8000-000000000001',
        createdAt: '2026-07-29T18:00:00Z',
        daraVersion: '0.1.0',
        txid: '000000000000000a',
        mainMigrationHead: 9,
        mediaMigrationHead: 8,
        referencedMediaCount: 3,
        referencedMediaBytes: 2048,
        availability: RemoteCheckpointAvailability.Restorable,
      },
    ],
    malformedObjectsIgnored: 0,
    backupSetId: '019f547b-6200-7000-8000-000000000010',
  })
  const { getByLabelText, getByRole, findByText } = render(
    <RecoveryWindow gateway={gateway} />,
  )

  fireEvent.click(getByRole('button', { name: /restore from backup/i }))
  fireEvent.change(getByLabelText('R2 account ID'), {
    target: { value: ACCOUNT_ID },
  })
  fireEvent.change(getByLabelText('Bucket'), {
    target: { value: 'dara-backups' },
  })
  fireEvent.change(getByLabelText('Access Key ID'), {
    target: { value: ACCESS_KEY_ID },
  })
  fireEvent.change(getByLabelText('Secret Access Key'), {
    target: { value: SECRET_ACCESS_KEY },
  })
  fireEvent.click(getByRole('button', { name: 'Find backups' }))

  expect(await findByText('Ready to restore')).toBeTruthy()
  expect(await findByText(/3 images/)).toBeTruthy()
  expect(
    getByRole('button', { name: /ready to restore/i }).classList.contains(
      'dara-button',
    ),
  ).toBe(true)
  expect(gateway.discover).toHaveBeenCalledWith({
    accountId: ACCOUNT_ID,
    jurisdiction: R2Jurisdiction.Default,
    bucket: 'dara-backups',
    credentials: {
      accessKeyId: ACCESS_KEY_ID,
      secretAccessKey: SECRET_ACCESS_KEY,
    },
  })

  fireEvent.click(getByRole('button', { name: 'Use different details' }))
  await waitFor(() => {
    expect((getByLabelText('Access Key ID') as HTMLInputElement).value).toBe('')
    expect(
      (getByLabelText('Secret Access Key') as HTMLInputElement).value,
    ).toBe('')
  })
})

test('restores only the explicitly selected restorable checkpoint', async () => {
  const gateway = gatewayFixture()
  gateway.discover.mockResolvedValueOnce({
    checkpoints: [
      {
        checkpointId: '019f547b-6200-7000-8000-000000000001',
        createdAt: '2026-07-29T18:00:00Z',
        daraVersion: '0.1.0',
        txid: '000000000000000a',
        mainMigrationHead: 9,
        mediaMigrationHead: 8,
        referencedMediaCount: 1,
        referencedMediaBytes: 2048,
        availability: RemoteCheckpointAvailability.Restorable,
      },
    ],
    malformedObjectsIgnored: 0,
    backupSetId: '019f547b-6200-7000-8000-000000000010',
  })
  gateway.restore.mockImplementationOnce(() => new Promise(() => {}))
  const { getByLabelText, getByRole, findByText } = render(
    <RecoveryWindow gateway={gateway} />,
  )

  fireEvent.click(getByRole('button', { name: /restore from backup/i }))
  for (const [label, value] of [
    ['R2 account ID', ACCOUNT_ID],
    ['Bucket', 'dara-backups'],
    ['Access Key ID', ACCESS_KEY_ID],
    ['Secret Access Key', SECRET_ACCESS_KEY],
  ] as const) {
    fireEvent.change(getByLabelText(label), { target: { value } })
  }
  fireEvent.click(getByRole('button', { name: 'Find backups' }))
  await findByText('Ready to restore')

  fireEvent.click(
    getByRole('button', { name: 'Restore selected backup' }),
  )

  expect(gateway.restore).toHaveBeenCalledWith({
    checkpointId: '019f547b-6200-7000-8000-000000000001',
  })
  expect(
    await findByText(/restoring and checking your databases and images/i),
  ).toBeTruthy()
})

function gatewayFixture() {
  return {
    loadLaunchContext: vi.fn(),
    startFresh: vi.fn(),
    discover: vi.fn(),
    restore: vi.fn(),
  } satisfies FreshInstallRecoveryGateway
}
