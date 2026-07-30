import {
  useId,
  useState,
  type FormEvent,
  type ReactNode,
} from 'react'
import {
  BackupErrorCode,
  R2Jurisdiction,
} from '../backup/index.ts'
import {
  validateR2ConnectionForm,
  type R2ConnectionFormErrors,
} from '../backup/r2-form-validation.ts'
import { DaraButton } from '../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../components/dara-button-types.ts'
import { DaraInput } from '../components/DaraInput.tsx'
import { DaraSelect } from '../components/DaraSelect.tsx'
import {
  RecoveryCommandErrorCode,
  RemoteCheckpointAvailability,
  tauriFreshInstallRecoveryGateway,
  type FreshInstallRecoveryGateway,
  type RecoveryCommandError,
  type RemoteCheckpointCatalog,
} from './index.ts'
import './recovery-window.css'

const RecoveryStep = {
  Choose: 'CHOOSE',
  Connect: 'CONNECT',
  Results: 'RESULTS',
} as const

type RecoveryStep = (typeof RecoveryStep)[keyof typeof RecoveryStep]

interface RecoveryForm {
  accountId: string
  jurisdiction: R2Jurisdiction
  bucket: string
  accessKeyId: string
  secretAccessKey: string
}

const jurisdictionOptions = [
  { label: 'Automatic', value: R2Jurisdiction.Default },
  { label: 'European Union', value: R2Jurisdiction.Eu },
  { label: 'FedRAMP', value: R2Jurisdiction.Fedramp },
] as const

interface RecoveryWindowProps {
  gateway?: FreshInstallRecoveryGateway
}

export function RecoveryWindow({
  gateway = tauriFreshInstallRecoveryGateway,
}: RecoveryWindowProps) {
  const [step, setStep] = useState<RecoveryStep>(RecoveryStep.Choose)
  const [form, setForm] = useState<RecoveryForm>(emptyForm())
  const [errors, setErrors] = useState<R2ConnectionFormErrors>({})
  const [catalog, setCatalog] = useState<RemoteCheckpointCatalog | null>(null)
  const [selectedCheckpointId, setSelectedCheckpointId] =
    useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [operationError, setOperationError] = useState<string | null>(null)
  const accountId = useId()
  const jurisdictionId = useId()
  const bucketId = useId()
  const accessKeyId = useId()
  const secretAccessKeyId = useId()

  const startFresh = async () => {
    setBusy(true)
    setOperationError(null)
    try {
      await gateway.startFresh()
    } catch (error) {
      setOperationError(recoveryErrorMessage(error))
      setBusy(false)
    }
  }

  const discover = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const nextErrors = validateR2ConnectionForm(form)
    setErrors(nextErrors)
    if (Object.keys(nextErrors).length > 0) {
      return
    }
    setBusy(true)
    setOperationError(null)
    try {
      const nextCatalog = await gateway.discover({
        accountId: form.accountId,
        jurisdiction: form.jurisdiction,
        bucket: form.bucket,
        credentials: {
          accessKeyId: form.accessKeyId,
          secretAccessKey: form.secretAccessKey,
        },
      })
      setCatalog(nextCatalog)
      setSelectedCheckpointId(
        nextCatalog.checkpoints.find(
          (checkpoint) =>
            checkpoint.availability ===
            RemoteCheckpointAvailability.Restorable,
        )?.checkpointId ?? null,
      )
      setStep(RecoveryStep.Results)
    } catch (error) {
      setOperationError(recoveryErrorMessage(error))
    } finally {
      setForm((current) => ({
        ...current,
        accessKeyId: '',
        secretAccessKey: '',
      }))
      setBusy(false)
    }
  }

  const restore = async () => {
    if (!selectedCheckpointId) {
      return
    }
    setBusy(true)
    setOperationError(null)
    try {
      await gateway.restore({ checkpointId: selectedCheckpointId })
    } catch (error) {
      setOperationError(recoveryErrorMessage(error))
      setBusy(false)
    }
  }

  const updateForm = <Field extends keyof RecoveryForm>(
    field: Field,
    value: RecoveryForm[Field],
  ) => {
    setForm((current) => ({ ...current, [field]: value }))
    setErrors((current) => ({ ...current, [field]: undefined }))
  }

  return (
    <main className="recovery-window">
      <section
        aria-labelledby="fresh-install-title"
        className="recovery-card"
      >
        <header className="recovery-header">
          <span>WELCOME TO DARA</span>
          <h1 id="fresh-install-title">
            {step === RecoveryStep.Choose
              ? 'How would you like to begin?'
              : step === RecoveryStep.Connect
                ? 'Find your off-site backup'
                : 'Choose a backup'}
          </h1>
          <p>
            {step === RecoveryStep.Choose
              ? 'This Mac does not have any Dara data yet.'
              : step === RecoveryStep.Connect
                ? 'Enter the Cloudflare R2 details that own your Dara backup.'
                : 'Dara found these complete checkpoints under its private backup location.'}
          </p>
        </header>

        {step === RecoveryStep.Choose && (
          <div className="recovery-choice-list">
            <DaraButton
              className="recovery-choice"
              disabled={busy}
              onClick={() => void startFresh()}
              size={DaraButtonSize.Custom}
              variant={DaraButtonVariant.Custom}
            >
              <strong>Start fresh</strong>
              <span>Create an empty Dara library on this Mac.</span>
            </DaraButton>
            <DaraButton
              className="recovery-choice"
              disabled={busy}
              onClick={() => {
                setOperationError(null)
                setStep(RecoveryStep.Connect)
              }}
              size={DaraButtonSize.Custom}
              variant={DaraButtonVariant.Custom}
            >
              <strong>Restore from backup</strong>
              <span>Find a complete Dara backup in your Cloudflare R2 bucket.</span>
            </DaraButton>
          </div>
        )}

        {step === RecoveryStep.Connect && (
          <form className="recovery-form" onSubmit={(event) => void discover(event)}>
            <RecoveryField
              error={errors.accountId}
              id={accountId}
              label="R2 account ID"
            >
              <DaraInput
                aria-describedby={describedBy(accountId, errors.accountId)}
                aria-invalid={Boolean(errors.accountId)}
                disabled={busy}
                id={accountId}
                onChange={(event) => updateForm('accountId', event.target.value)}
                value={form.accountId}
              />
            </RecoveryField>
            <RecoveryField id={jurisdictionId} label="Jurisdiction">
              <DaraSelect
                ariaLabel="Jurisdiction"
                disabled={busy}
                id={jurisdictionId}
                onSelect={(value) => updateForm('jurisdiction', value)}
                options={jurisdictionOptions}
                value={form.jurisdiction}
              />
            </RecoveryField>
            <RecoveryField error={errors.bucket} id={bucketId} label="Bucket">
              <DaraInput
                aria-describedby={describedBy(bucketId, errors.bucket)}
                aria-invalid={Boolean(errors.bucket)}
                disabled={busy}
                id={bucketId}
                onChange={(event) => updateForm('bucket', event.target.value)}
                value={form.bucket}
              />
            </RecoveryField>
            <RecoveryField
              error={errors.accessKeyId}
              id={accessKeyId}
              label="Access Key ID"
            >
              <DaraInput
                aria-describedby={describedBy(accessKeyId, errors.accessKeyId)}
                aria-invalid={Boolean(errors.accessKeyId)}
                disabled={busy}
                id={accessKeyId}
                onChange={(event) =>
                  updateForm('accessKeyId', event.target.value)
                }
                type="password"
                value={form.accessKeyId}
              />
            </RecoveryField>
            <RecoveryField
              error={errors.secretAccessKey}
              id={secretAccessKeyId}
              label="Secret Access Key"
            >
              <DaraInput
                aria-describedby={describedBy(
                  secretAccessKeyId,
                  errors.secretAccessKey,
                )}
                aria-invalid={Boolean(errors.secretAccessKey)}
                disabled={busy}
                id={secretAccessKeyId}
                onChange={(event) =>
                  updateForm('secretAccessKey', event.target.value)
                }
                type="password"
                value={form.secretAccessKey}
              />
            </RecoveryField>
            <p className="recovery-path-note">
              Dara will look in <strong>dara/primary</strong>. Your credentials
              stay in this app and are not saved unless you complete a restore.
            </p>
            <div className="recovery-actions">
              <DaraButton
                disabled={busy}
                onClick={() => setStep(RecoveryStep.Choose)}
                variant={DaraButtonVariant.Ghost}
              >
                Back
              </DaraButton>
              <DaraButton
                disabled={busy}
                type="submit"
                variant={DaraButtonVariant.Accent}
              >
                {busy ? 'Checking R2…' : 'Find backups'}
              </DaraButton>
            </div>
          </form>
        )}

        {step === RecoveryStep.Results && catalog && (
          <div className="recovery-results">
            {catalog.checkpoints.length === 0 ? (
              <div className="recovery-empty">
                <strong>No complete backups found</strong>
                <span>
                  This bucket does not currently contain a Dara backup under
                  dara/primary.
                </span>
              </div>
            ) : (
              <ul className="recovery-checkpoint-list">
                {catalog.checkpoints.map((checkpoint) => {
                  const restorable =
                    checkpoint.availability ===
                    RemoteCheckpointAvailability.Restorable
                  return (
                    <li key={checkpoint.checkpointId}>
                      <DaraButton
                        aria-pressed={
                          selectedCheckpointId === checkpoint.checkpointId
                        }
                        className="recovery-checkpoint"
                        disabled={!restorable || busy}
                        onClick={() =>
                          setSelectedCheckpointId(checkpoint.checkpointId)
                        }
                        size={DaraButtonSize.Custom}
                        variant={DaraButtonVariant.Custom}
                      >
                        <div>
                          <strong>{formatDate(checkpoint.createdAt)}</strong>
                          <span>
                            Dara {checkpoint.daraVersion} ·{' '}
                            {formatBytes(checkpoint.referencedMediaBytes)} in{' '}
                            {checkpoint.referencedMediaCount}{' '}
                            {checkpoint.referencedMediaCount === 1
                              ? 'image'
                              : 'images'}
                          </span>
                        </div>
                        <span className={restorable ? 'ready' : 'unavailable'}>
                          {restorable ? 'Ready to restore' : 'Unavailable'}
                        </span>
                      </DaraButton>
                    </li>
                  )
                })}
              </ul>
            )}
            {catalog.malformedObjectsIgnored > 0 && (
              <p className="recovery-catalog-note">
                Dara safely ignored {catalog.malformedObjectsIgnored} invalid
                backup{' '}
                {catalog.malformedObjectsIgnored === 1 ? 'record' : 'records'}.
              </p>
            )}
            <div className="recovery-actions">
              <DaraButton
                disabled={busy}
                onClick={() => {
                  setCatalog(null)
                  setSelectedCheckpointId(null)
                  setStep(RecoveryStep.Connect)
                }}
                variant={DaraButtonVariant.Ghost}
              >
                Use different details
              </DaraButton>
              <DaraButton
                disabled={!selectedCheckpointId || busy}
                onClick={() => void restore()}
                variant={DaraButtonVariant.Accent}
              >
                {busy ? 'Restoring…' : 'Restore selected backup'}
              </DaraButton>
            </div>
            {busy && (
              <p aria-live="polite" className="recovery-restore-progress">
                Restoring and checking your databases and images. Dara will
                reopen when everything is safe.
              </p>
            )}
          </div>
        )}

        {busy && step === RecoveryStep.Choose && (
          <p aria-live="polite" className="recovery-notice">
            Creating your Dara library…
          </p>
        )}
        {operationError && (
          <p aria-live="assertive" className="recovery-error" role="alert">
            {operationError}
          </p>
        )}
      </section>
    </main>
  )
}

function RecoveryField({
  children,
  error,
  id,
  label,
}: {
  children: ReactNode
  error?: string
  id: string
  label: string
}) {
  return (
    <div className="recovery-field">
      <label htmlFor={id}>{label}</label>
      {children}
      {error && <span id={`${id}-error`}>{error}</span>}
    </div>
  )
}

function emptyForm(): RecoveryForm {
  return {
    accountId: '',
    jurisdiction: R2Jurisdiction.Default,
    bucket: '',
    accessKeyId: '',
    secretAccessKey: '',
  }
}

function describedBy(id: string, error?: string): string | undefined {
  return error ? `${id}-error` : undefined
}

function recoveryErrorMessage(error: unknown): string {
  if (!isRecoveryCommandError(error)) {
    return error instanceof Error
      ? error.message
      : 'Dara could not complete that recovery step.'
  }
  if (error.code !== RecoveryCommandErrorCode.BackupFailed) {
    return error.message
  }
  const messages: Partial<Record<BackupErrorCode, string>> = {
    [BackupErrorCode.NetworkOffline]:
      'This Mac appears to be offline. Reconnect and try again.',
    [BackupErrorCode.NetworkTimeout]:
      'Cloudflare R2 did not respond in time. Try again.',
    [BackupErrorCode.AuthenticationRejected]:
      'Cloudflare rejected the R2 credentials.',
    [BackupErrorCode.AuthorizationRejected]:
      'Those credentials cannot read this R2 bucket.',
    [BackupErrorCode.KeychainUnavailable]:
      'Dara could not save the R2 credentials in Keychain.',
    [BackupErrorCode.CheckpointNotFound]:
      'No complete Dara backup was found in this bucket.',
    [BackupErrorCode.LitestreamUnavailable]:
      'Dara could not start its backup helper.',
    [BackupErrorCode.ExactTxidUnavailable]:
      'That exact database backup is no longer available in R2.',
    [BackupErrorCode.PrefixIdentityMismatch]:
      'The data under dara/primary does not belong to one consistent Dara backup.',
    [BackupErrorCode.RestoreValidationFailed]:
      'Dara could not validate and install that backup safely.',
    [BackupErrorCode.RemoteMediaMissing]:
      'An image required by this backup is missing from R2.',
    [BackupErrorCode.RemoteMediaCorrupt]:
      'An image in this backup failed its integrity check.',
  }
  return (
    (error.backupErrorCode && messages[error.backupErrorCode]) ??
    error.message
  )
}

function isRecoveryCommandError(
  error: unknown,
): error is RecoveryCommandError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    'message' in error
  )
}

function formatDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? 'Date unavailable'
    : new Intl.DateTimeFormat(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short',
      }).format(date)
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
