import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type Dispatch,
  type FormEvent,
  type ReactNode,
  type Ref,
  type SetStateAction,
} from 'react'
import {
  BackupErrorCode,
  CheckpointBackupPhase,
  CredentialAvailability,
  MediaBackupPhase,
  OffsiteBackupOperationKind,
  OffsiteBackupProgressPhase,
  R2Jurisdiction,
  RelationalBackupPhase,
  RestoreDrillOutcome,
  tauriOffsiteBackupGateway,
  type OffsiteBackupGateway,
  type OffsiteBackupOperation,
  type OffsiteBackupProgress,
  type OffsiteBackupStatus,
  type OffsiteBackupTarget,
} from '../../backup/index.ts'
import { DaraButton } from '../../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../components/dara-button-types.ts'
import { DaraInput } from '../../components/DaraInput.tsx'
import { DaraSelect } from '../../components/DaraSelect.tsx'
import { ConfirmationDialog } from './ConfirmationDialog.tsx'
import { ConfirmationDialogInitialFocus } from './confirmation-dialog.ts'

const DEFAULT_PREFIX = 'dara/primary'
const STATUS_REFRESH_MILLIS = 5_000

const BackupFormMode = {
  Configure: 'CONFIGURE',
  ReplaceCredentials: 'REPLACE_CREDENTIALS',
  ChangeTarget: 'CHANGE_TARGET',
} as const

type BackupFormMode = (typeof BackupFormMode)[keyof typeof BackupFormMode]

const BackupConfirmation = {
  ChangeTarget: 'CHANGE_TARGET',
  Disable: 'DISABLE',
  RemoveCredentials: 'REMOVE_CREDENTIALS',
  TakeOver: 'TAKE_OVER',
} as const

type BackupConfirmation =
  (typeof BackupConfirmation)[keyof typeof BackupConfirmation]

const BackupStatusTone = {
  Healthy: 'healthy',
  Neutral: 'neutral',
  Warning: 'warning',
} as const

type BackupStatusTone =
  (typeof BackupStatusTone)[keyof typeof BackupStatusTone]

const jurisdictionOptions = [
  { label: 'Automatic', value: R2Jurisdiction.Default },
  { label: 'European Union', value: R2Jurisdiction.Eu },
  { label: 'FedRAMP', value: R2Jurisdiction.Fedramp },
] as const

interface BackupForm {
  accountId: string
  jurisdiction: R2Jurisdiction
  bucket: string
  prefix: string
  accessKeyId: string
  secretAccessKey: string
}

interface BackupFormErrors {
  accountId?: string
  bucket?: string
  prefix?: string
  accessKeyId?: string
  secretAccessKey?: string
}

interface OffsiteBackupSectionProps {
  disabled?: boolean
  gateway?: OffsiteBackupGateway
  onBusyChange: (busy: boolean) => void
}

export function OffsiteBackupSection({
  disabled = false,
  gateway = tauriOffsiteBackupGateway,
  onBusyChange,
}: OffsiteBackupSectionProps) {
  const [status, setStatus] = useState<OffsiteBackupStatus | null>(null)
  const [loadingError, setLoadingError] = useState<string | null>(null)
  const [formMode, setFormMode] = useState<BackupFormMode>(
    BackupFormMode.Configure,
  )
  const [form, setForm] = useState<BackupForm>(emptyForm())
  const [formErrors, setFormErrors] = useState<BackupFormErrors>({})
  const [operation, setOperation] =
    useState<OffsiteBackupOperationKind | null>(null)
  const [progress, setProgress] = useState<OffsiteBackupProgress | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [operationError, setOperationError] = useState<string | null>(null)
  const [confirmation, setConfirmation] =
    useState<BackupConfirmation | null>(null)
  const replaceButtonRef = useRef<HTMLButtonElement>(null)
  const changeTargetButtonRef = useRef<HTMLButtonElement>(null)
  const changeTargetSubmitRef = useRef<HTMLButtonElement>(null)
  const disableButtonRef = useRef<HTMLButtonElement>(null)
  const removeButtonRef = useRef<HTMLButtonElement>(null)
  const takeoverButtonRef = useRef<HTMLButtonElement>(null)
  const targetFormDirtyRef = useRef(false)
  const accountId = useId()
  const bucketId = useId()
  const prefixId = useId()
  const accessKeyId = useId()
  const secretAccessKeyId = useId()

  const applyStatus = useCallback((next: OffsiteBackupStatus) => {
    setStatus(next)
    setLoadingError(null)
    if (next.target && !targetFormDirtyRef.current) {
      setForm((current) => ({
        ...current,
        accountId: next.target?.accountId ?? '',
        jurisdiction: next.target?.jurisdiction ?? R2Jurisdiction.Default,
        bucket: next.target?.bucket ?? '',
        prefix: next.target?.prefix ?? DEFAULT_PREFIX,
      }))
    }
  }, [])

  const reload = useCallback(async () => {
    try {
      applyStatus(await gateway.loadStatus())
    } catch (error) {
      setLoadingError(errorMessage(error))
    }
  }, [applyStatus, gateway])

  useEffect(() => {
    void reload()
  }, [reload])

  const backupEnabled = status?.enabled ?? false
  const activeStatusOperationId = status?.activeOperation?.operationId

  useEffect(() => {
    if (!backupEnabled && activeStatusOperationId === undefined) {
      return
    }
    const interval = window.setInterval(() => {
      void reload()
    }, STATUS_REFRESH_MILLIS)
    return () => window.clearInterval(interval)
  }, [activeStatusOperationId, backupEnabled, reload])

  useEffect(() => {
    let disposed = false
    let stopListening: (() => void) | undefined
    void gateway.listenToProgress((next) => {
      if (disposed) {
        return
      }
      setProgress(next)
      setOperation(next.phase === OffsiteBackupProgressPhase.Complete ||
          next.phase === OffsiteBackupProgressPhase.Failed
        ? null
        : next.operation)
      if (
        next.phase === OffsiteBackupProgressPhase.Complete ||
        next.phase === OffsiteBackupProgressPhase.Failed
      ) {
        void reload()
      }
    }).then((unlisten) => {
      if (disposed) {
        unlisten()
      } else {
        stopListening = unlisten
      }
    })
    return () => {
      disposed = true
      stopListening?.()
    }
  }, [gateway, reload])

  useEffect(
    () => () => {
      onBusyChange(false)
    },
    [onBusyChange],
  )

  const runOperation = async (
    kind: OffsiteBackupOperationKind,
    invoke: () => Promise<OffsiteBackupOperation>,
    success: string,
    returnFocus?: () => void,
  ) => {
    setOperation(kind)
    setProgress(null)
    setNotice(null)
    setOperationError(null)
    setConfirmation(null)
    onBusyChange(true)
    try {
      const accepted = await invoke()
      await reload()
      if (accepted.reused) {
        setNotice('That backup task is already running.')
      } else {
        setNotice(success)
        targetFormDirtyRef.current = false
        setFormMode(BackupFormMode.Configure)
      }
    } catch (error) {
      setOperationError(errorMessage(error))
      await reload()
    } finally {
      clearCredentials(setForm)
      setOperation(null)
      onBusyChange(false)
      if (returnFocus) {
        requestAnimationFrame(returnFocus)
      }
    }
  }

  const submitConfiguration = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const errors = validateForm(form, formMode)
    setFormErrors(errors)
    if (Object.keys(errors).length > 0) {
      return
    }
    const credentials = {
      accessKeyId: form.accessKeyId,
      secretAccessKey: form.secretAccessKey,
    }
    if (formMode === BackupFormMode.ReplaceCredentials) {
      void runOperation(
        OffsiteBackupOperationKind.ReplaceCredentials,
        () => gateway.replaceCredentials({ credentials }),
        'The new credentials were tested and saved.',
        () => replaceButtonRef.current?.focus(),
      )
      return
    }
    const target = targetFromForm(form)
    if (formMode === BackupFormMode.ChangeTarget) {
      if (targetsMatch(status?.target ?? null, target)) {
        setOperationError('Change at least one R2 target field.')
        return
      }
      setConfirmation(BackupConfirmation.ChangeTarget)
      return
    }
    if (status?.configured && !targetsMatch(status.target, target)) {
      setConfirmation(BackupConfirmation.ChangeTarget)
      return
    }
    void runOperation(
      OffsiteBackupOperationKind.TestAndEnable,
      () => gateway.testAndEnable({ credentials, target }),
      'Off-site backup is enabled. Dara is building its first complete backup.',
    )
  }

  const activeOperation = operation ?? status?.activeOperation?.operation ?? null
  const controlsDisabled = disabled || activeOperation !== null
  const enabled = status?.enabled === true

  return (
    <section className="setting-section offsite-backup-section">
      <div className="setting-section-heading">
        <h2>Off-site backup</h2>
        <p>
          Keep a recoverable copy of Dara in your private Cloudflare R2 bucket.
        </p>
      </div>
      <div className="setting-section-body">
        {loadingError && status === null ? (
          <div className="offsite-backup-load-error" role="alert">
            <span>{loadingError}</span>
            <DaraButton onClick={() => void reload()} type="button">
              Try again
            </DaraButton>
          </div>
        ) : status === null ? (
          <p className="offsite-backup-loading">Loading backup status…</p>
        ) : (
          <>
            <div className="offsite-backup-intro">
              <div>
                <strong>{enabled ? 'Backup is on' : 'Backup is off'}</strong>
                <span>
                  Dara always works from this Mac. A network or backup problem
                  will not block normal use.
                </span>
              </div>
              <span className={`backup-state-pill ${enabled ? 'on' : 'off'}`}>
                {enabled ? 'On' : 'Off'}
              </span>
            </div>

            {!enabled || formMode !== BackupFormMode.Configure ? (
              <BackupConfigurationForm
                accountId={accountId}
                accessKeyId={accessKeyId}
                controlsDisabled={controlsDisabled}
                errors={formErrors}
                form={form}
                configured={status.configured}
                mode={formMode}
                onCancel={
                  status.configured && formMode !== BackupFormMode.Configure
                    ? () => {
                        targetFormDirtyRef.current = false
                        clearCredentials(setForm)
                        const target = status.target
                        if (target) {
                          setForm((current) =>
                            applyTargetToForm(current, target),
                          )
                        }
                        setFormErrors({})
                        setFormMode(BackupFormMode.Configure)
                        requestAnimationFrame(() => {
                          if (formMode === BackupFormMode.ReplaceCredentials) {
                            replaceButtonRef.current?.focus()
                          } else {
                            changeTargetButtonRef.current?.focus()
                          }
                        })
                      }
                    : undefined
                }
                onChange={(field, value) => {
                  if (isTargetFormField(field)) {
                    targetFormDirtyRef.current = true
                  }
                  setForm((current) => ({ ...current, [field]: value }))
                  setFormErrors((current) => ({ ...current, [field]: undefined }))
                  setOperationError(null)
                }}
                onSubmit={submitConfiguration}
                prefixId={prefixId}
                bucketId={bucketId}
                secretAccessKeyId={secretAccessKeyId}
                submitRef={
                  changeTargetSubmitRef
                }
              />
            ) : (
              <EnabledBackupStatus status={status} />
            )}

            {enabled && formMode === BackupFormMode.Configure && (
              <div className="offsite-backup-actions">
                <div className="offsite-backup-primary-actions">
                  <DaraButton
                    disabled={controlsDisabled}
                    onClick={() =>
                      void runOperation(
                        OffsiteBackupOperationKind.BackupNow,
                        () => gateway.backupNow(),
                        'A complete recoverable backup was created.',
                      )
                    }
                    type="button"
                    variant={DaraButtonVariant.Accent}
                  >
                    {activeOperation === OffsiteBackupOperationKind.BackupNow
                      ? 'Backing up…'
                      : 'Back up now'}
                  </DaraButton>
                  <DaraButton
                    disabled={
                      controlsDisabled ||
                      status.checkpoint.lastCompleteCheckpointId === null
                    }
                    onClick={() =>
                      void runOperation(
                        OffsiteBackupOperationKind.RestoreDrill,
                        () => gateway.runRestoreDrill(),
                        'Restore drill passed. The latest complete backup was verified.',
                      )
                    }
                    type="button"
                  >
                    {activeOperation === OffsiteBackupOperationKind.RestoreDrill
                      ? 'Testing restore…'
                      : 'Run restore drill'}
                  </DaraButton>
                </div>
                <div className="offsite-backup-secondary-actions">
                  <DaraButton
                    disabled={controlsDisabled}
                    onClick={() => {
                      setFormMode(BackupFormMode.ReplaceCredentials)
                      setOperationError(null)
                    }}
                    ref={replaceButtonRef}
                    size={DaraButtonSize.Mini}
                    type="button"
                    variant={DaraButtonVariant.Ghost}
                  >
                    Replace credentials
                  </DaraButton>
                  <DaraButton
                    disabled={controlsDisabled}
                    onClick={() => {
                      targetFormDirtyRef.current = false
                      clearCredentials(setForm)
                      const target = status.target
                      if (target) {
                        setForm((current) =>
                          applyTargetToForm(current, target),
                        )
                      }
                      setFormMode(BackupFormMode.ChangeTarget)
                      setOperationError(null)
                    }}
                    ref={changeTargetButtonRef}
                    size={DaraButtonSize.Mini}
                    type="button"
                    variant={DaraButtonVariant.Ghost}
                  >
                    Change target
                  </DaraButton>
                  {status.takeoverAvailable && (
                    <DaraButton
                      disabled={controlsDisabled}
                      onClick={() =>
                        setConfirmation(BackupConfirmation.TakeOver)
                      }
                      ref={takeoverButtonRef}
                      size={DaraButtonSize.Mini}
                      type="button"
                      variant={DaraButtonVariant.Ghost}
                    >
                      Take over restored backup
                    </DaraButton>
                  )}
                  <DaraButton
                    disabled={controlsDisabled}
                    onClick={() =>
                      setConfirmation(BackupConfirmation.Disable)
                    }
                    ref={disableButtonRef}
                    size={DaraButtonSize.Mini}
                    type="button"
                    variant={DaraButtonVariant.Ghost}
                  >
                    Disable backup
                  </DaraButton>
                  <DaraButton
                    disabled={controlsDisabled}
                    onClick={() =>
                      setConfirmation(BackupConfirmation.RemoveCredentials)
                    }
                    ref={removeButtonRef}
                    size={DaraButtonSize.Mini}
                    type="button"
                    variant={DaraButtonVariant.Ghost}
                  >
                    Remove saved credentials
                  </DaraButton>
                </div>
              </div>
            )}
            {!enabled &&
              status.configured &&
              formMode === BackupFormMode.Configure &&
              status.credentials !== CredentialAvailability.Missing && (
                <div className="offsite-backup-actions">
                  <div className="offsite-backup-secondary-actions">
                    {status.takeoverAvailable && (
                      <DaraButton
                        disabled={controlsDisabled}
                        onClick={() =>
                          setConfirmation(BackupConfirmation.TakeOver)
                        }
                        ref={takeoverButtonRef}
                        size={DaraButtonSize.Mini}
                        type="button"
                        variant={DaraButtonVariant.Ghost}
                      >
                        Take over restored backup
                      </DaraButton>
                    )}
                    <DaraButton
                      disabled={controlsDisabled}
                      onClick={() =>
                        setConfirmation(BackupConfirmation.RemoveCredentials)
                      }
                      ref={removeButtonRef}
                      size={DaraButtonSize.Mini}
                      type="button"
                      variant={DaraButtonVariant.Ghost}
                    >
                      Remove saved credentials
                    </DaraButton>
                  </div>
                </div>
              )}

            {status.credentials !== CredentialAvailability.Present &&
              status.configured && (
                <p className="offsite-backup-warning" role="status">
                  {status.credentials === CredentialAvailability.Missing
                    ? 'R2 credentials are not saved on this Mac.'
                    : 'macOS Keychain is currently unavailable.'}
                </p>
              )}
            <p className="offsite-backup-privacy">
              Backup contents are not encrypted by Dara before upload. Your R2
              credentials and Cloudflare’s private-bucket controls protect
              access.
            </p>
            <div aria-atomic="true" aria-live="polite" className="offsite-backup-live">
              {activeOperation && (
                <p role="status">
                  {progressMessage(
                    progress?.operation === activeOperation
                      ? progress.phase
                      : OffsiteBackupProgressPhase.ValidatingConfig,
                  )}
                </p>
              )}
              {notice && <p className="success" role="status">{notice}</p>}
              {operationError && <p className="error" role="alert">{operationError}</p>}
            </div>
          </>
        )}
      </div>

      {confirmation === BackupConfirmation.ChangeTarget && (
        <ConfirmationDialog
          busy={operation === OffsiteBackupOperationKind.ChangeTarget}
          confirmLabel={
            operation === OffsiteBackupOperationKind.ChangeTarget
              ? 'Changing…'
              : 'Change target'
          }
          initialFocus={ConfirmationDialogInitialFocus.Cancel}
          onCancel={() => {
            if (operation === null) {
              setConfirmation(null)
              requestAnimationFrame(() =>
                changeTargetSubmitRef.current?.focus(),
              )
            }
          }}
          onConfirm={() => {
            const credentials = {
              accessKeyId: form.accessKeyId,
              secretAccessKey: form.secretAccessKey,
            }
            const target = targetFromForm(form)
            void runOperation(
              OffsiteBackupOperationKind.ChangeTarget,
              () => gateway.changeTarget({ credentials, target }),
              'The new target is enabled. Dara is building its first complete backup.',
              () => {
                const focusTarget =
                  changeTargetSubmitRef.current ??
                  changeTargetButtonRef.current
                focusTarget?.focus()
              },
            )
          }}
          title="Change off-site backup target?"
        >
          <p>
            Dara will create a separate backup at the new target. It will not
            delete or redirect the existing backup in R2.
          </p>
        </ConfirmationDialog>
      )}

      {confirmation === BackupConfirmation.Disable && (
        <ConfirmationDialog
          busy={operation === OffsiteBackupOperationKind.Disable}
          confirmLabel={
            operation === OffsiteBackupOperationKind.Disable
              ? 'Disabling…'
              : 'Disable backup'
          }
          confirmVariant={DaraButtonVariant.Danger}
          initialFocus={ConfirmationDialogInitialFocus.Cancel}
          onCancel={() => {
            if (operation === null) {
              setConfirmation(null)
              requestAnimationFrame(() => disableButtonRef.current?.focus())
            }
          }}
          onConfirm={() =>
            void runOperation(
              OffsiteBackupOperationKind.Disable,
              () => gateway.disable(),
              'Backup is off. Existing remote backups and saved credentials were kept.',
              () => disableButtonRef.current?.focus(),
            )
          }
          title="Disable off-site backup?"
        >
          <p>
            Dara will stop sending new changes to R2. Existing remote backups
            and saved credentials will remain.
          </p>
        </ConfirmationDialog>
      )}

      {confirmation === BackupConfirmation.RemoveCredentials && (
        <ConfirmationDialog
          busy={operation === OffsiteBackupOperationKind.RemoveCredentials}
          confirmLabel={
            operation === OffsiteBackupOperationKind.RemoveCredentials
              ? 'Removing…'
              : 'Disable and remove'
          }
          confirmVariant={DaraButtonVariant.Danger}
          initialFocus={ConfirmationDialogInitialFocus.Cancel}
          onCancel={() => {
            if (operation === null) {
              setConfirmation(null)
              requestAnimationFrame(() => removeButtonRef.current?.focus())
            }
          }}
          onConfirm={() =>
            void runOperation(
              OffsiteBackupOperationKind.RemoveCredentials,
              () => gateway.removeCredentials(),
              'Backup is off and its credentials were removed from this Mac. Remote data was kept.',
              () => removeButtonRef.current?.focus(),
            )
          }
          title="Remove saved R2 credentials?"
        >
          <p>
            Backup will be disabled first. Credentials will be removed from
            macOS Keychain, but Dara will not delete anything from R2.
          </p>
        </ConfirmationDialog>
      )}

      {confirmation === BackupConfirmation.TakeOver && (
        <ConfirmationDialog
          busy={operation === OffsiteBackupOperationKind.TakeOver}
          confirmLabel={
            operation === OffsiteBackupOperationKind.TakeOver
              ? 'Taking over…'
              : 'Take over backup'
          }
          confirmVariant={DaraButtonVariant.Danger}
          initialFocus={ConfirmationDialogInitialFocus.Cancel}
          onCancel={() => {
            if (operation === null) {
              setConfirmation(null)
              requestAnimationFrame(() => takeoverButtonRef.current?.focus())
            }
          }}
          onConfirm={() =>
            void runOperation(
              OffsiteBackupOperationKind.TakeOver,
              () => gateway.takeOverRestoredBackup(),
              'This Mac now owns the backup. Dara is building a new complete checkpoint.',
              () => takeoverButtonRef.current?.focus(),
            )
          }
          title="Take over this restored backup?"
        >
          <p>
            Continue only if the old Dara installation will no longer write to
            this backup. Changes made separately on two Macs cannot be merged.
          </p>
        </ConfirmationDialog>
      )}
    </section>
  )
}

function BackupConfigurationForm({
  accountId,
  accessKeyId,
  bucketId,
  configured,
  controlsDisabled,
  errors,
  form,
  mode,
  onCancel,
  onChange,
  onSubmit,
  prefixId,
  secretAccessKeyId,
  submitRef,
}: {
  accountId: string
  accessKeyId: string
  bucketId: string
  configured: boolean
  controlsDisabled: boolean
  errors: BackupFormErrors
  form: BackupForm
  mode: BackupFormMode
  onCancel?: () => void
  onChange: (field: keyof BackupForm, value: string) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  prefixId: string
  secretAccessKeyId: string
  submitRef?: Ref<HTMLButtonElement>
}) {
  const credentialsOnly = mode === BackupFormMode.ReplaceCredentials
  return (
    <form className="offsite-backup-form" onSubmit={onSubmit}>
      {!credentialsOnly && (
        <>
          <BackupField
            error={errors.accountId}
            id={accountId}
            label="Account ID"
          >
            <DaraInput
              aria-describedby={errors.accountId ? `${accountId}-error` : undefined}
              aria-invalid={Boolean(errors.accountId)}
              disabled={controlsDisabled}
              id={accountId}
              onChange={(event) => onChange('accountId', event.target.value)}
              type="password"
              value={form.accountId}
            />
          </BackupField>
          <div className="offsite-backup-field">
            <label id={`${accountId}-jurisdiction-label`}>Jurisdiction</label>
            <DaraSelect
              ariaLabel="R2 jurisdiction"
              disabled={controlsDisabled}
              menuHeight={130}
              menuWidth={190}
              onSelect={(value) => onChange('jurisdiction', value)}
              options={jurisdictionOptions}
              value={form.jurisdiction}
            />
          </div>
          <BackupField error={errors.bucket} id={bucketId} label="Bucket">
            <DaraInput
              aria-describedby={errors.bucket ? `${bucketId}-error` : undefined}
              aria-invalid={Boolean(errors.bucket)}
              disabled={controlsDisabled}
              id={bucketId}
              onChange={(event) => onChange('bucket', event.target.value)}
              value={form.bucket}
            />
          </BackupField>
          <BackupField error={errors.prefix} id={prefixId} label="Prefix">
            <DaraInput
              aria-describedby={errors.prefix ? `${prefixId}-error` : undefined}
              aria-invalid={Boolean(errors.prefix)}
              disabled={controlsDisabled}
              id={prefixId}
              onChange={(event) => onChange('prefix', event.target.value)}
              value={form.prefix}
            />
          </BackupField>
        </>
      )}
      <BackupField
        error={errors.accessKeyId}
        id={accessKeyId}
        label="Access Key ID"
      >
        <DaraInput
          aria-describedby={
            errors.accessKeyId ? `${accessKeyId}-error` : undefined
          }
          aria-invalid={Boolean(errors.accessKeyId)}
          disabled={controlsDisabled}
          id={accessKeyId}
          onChange={(event) => onChange('accessKeyId', event.target.value)}
          type="password"
          value={form.accessKeyId}
        />
      </BackupField>
      <BackupField
        error={errors.secretAccessKey}
        id={secretAccessKeyId}
        label="Secret Access Key"
      >
        <DaraInput
          aria-describedby={
            errors.secretAccessKey
              ? `${secretAccessKeyId}-error`
              : undefined
          }
          aria-invalid={Boolean(errors.secretAccessKey)}
          disabled={controlsDisabled}
          id={secretAccessKeyId}
          onChange={(event) => onChange('secretAccessKey', event.target.value)}
          type="password"
          value={form.secretAccessKey}
        />
      </BackupField>
      <div className="offsite-backup-form-actions">
        {onCancel && (
          <DaraButton
            disabled={controlsDisabled}
            onClick={onCancel}
            type="button"
            variant={DaraButtonVariant.Ghost}
          >
            Cancel
          </DaraButton>
        )}
        <DaraButton
          disabled={controlsDisabled}
          ref={submitRef}
          type="submit"
          variant={DaraButtonVariant.Accent}
        >
          {mode === BackupFormMode.ReplaceCredentials
            ? 'Test and replace credentials'
            : mode === BackupFormMode.ChangeTarget
              ? 'Test and change target'
              : configured
                ? 'Test and re-enable backup'
              : 'Test and enable backup'}
        </DaraButton>
      </div>
    </form>
  )
}

function BackupField({
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
    <div className="offsite-backup-field">
      <label htmlFor={id}>{label}</label>
      {children}
      {error && (
        <span className="offsite-backup-field-error" id={`${id}-error`}>
          {error}
        </span>
      )}
    </div>
  )
}

function EnabledBackupStatus({ status }: { status: OffsiteBackupStatus }) {
  return (
    <dl className="offsite-backup-status-list">
      <BackupStatusItem
        detail={`Prefix ${status.target?.prefix ?? 'unavailable'} · ${jurisdictionLabel(status.target?.jurisdiction)}`}
        label="R2 target"
        state={status.target?.bucket ?? 'Unavailable'}
        tone={BackupStatusTone.Neutral}
      />
      <BackupStatusItem
        detail={
          `${status.relational.latestLocalTxid ? `Local database point ${status.relational.latestLocalTxid}. ` : ''}${
            status.relational.lastRemoteConfirmedAt
              ? `R2 confirmed changes ${formatDate(status.relational.lastRemoteConfirmedAt)}.`
              : 'R2 has not confirmed relational changes yet.'
          }`
        }
        label="Relational replication"
        state={relationalLabel(status.relational.phase)}
        tone={
          status.relational.phase === RelationalBackupPhase.Running
            ? BackupStatusTone.Healthy
            : BackupStatusTone.Warning
        }
      />
      <BackupStatusItem
        detail={`${status.media.verifiedCount} verified (${formatBytes(status.media.verifiedBytes)}) · ${status.media.pendingCount} pending (${formatBytes(status.media.pendingBytes)}) · ${status.media.retryWaitCount} retrying · ${status.media.blockedCount} blocked`}
        label="Media"
        state={mediaLabel(status.media.phase)}
        tone={
          status.media.phase === MediaBackupPhase.Idle
            ? BackupStatusTone.Healthy
            : BackupStatusTone.Warning
        }
      />
      <BackupStatusItem
        detail={
          status.checkpoint.lastCompleteCheckpointId &&
          status.checkpoint.lastCompleteAt
            ? `${formatDate(status.checkpoint.lastCompleteAt)} · checkpoint ${abbreviate(status.checkpoint.lastCompleteCheckpointId)}`
            : 'Relational data and media may still be syncing, but there is no complete recoverable checkpoint yet.'
        }
        label="Recoverable backup"
        state={
          status.checkpoint.lastCompleteCheckpointId
            ? checkpointLabel(status.checkpoint.phase)
            : 'Not ready'
        }
        tone={
          status.checkpoint.lastCompleteCheckpointId
            ? BackupStatusTone.Healthy
            : BackupStatusTone.Warning
        }
      />
      <BackupStatusItem
        detail={
          status.lastRestoreDrillError
            ? 'The saved restore-drill result could not be read safely.'
            : status.lastRestoreDrill
            ? `${status.lastRestoreDrillAt ? formatDate(status.lastRestoreDrillAt) : 'Time unavailable'} · ${formatDuration(status.lastRestoreDrill.durationMs)}`
            : 'No restore drill has been run on this Mac.'
        }
        label="Last restore drill"
        state={
          status.lastRestoreDrill?.outcome === RestoreDrillOutcome.Success
            ? 'Passed'
            : status.lastRestoreDrillError
              ? 'Unavailable'
            : status.lastRestoreDrill
              ? 'Failed'
              : 'Not run'
        }
        tone={
          status.lastRestoreDrill?.outcome === RestoreDrillOutcome.Success
            ? BackupStatusTone.Healthy
            : status.lastRestoreDrillError
              ? BackupStatusTone.Warning
            : BackupStatusTone.Neutral
        }
      />
    </dl>
  )
}

function BackupStatusItem({
  detail,
  label,
  state,
  tone,
}: {
  detail: string
  label: string
  state: string
  tone: BackupStatusTone
}) {
  return (
    <div className={`offsite-backup-status-item ${tone}`}>
      <dt>{label}</dt>
      <dd>
        <strong>{state}</strong>
        <span>{detail}</span>
      </dd>
    </div>
  )
}

function emptyForm(): BackupForm {
  return {
    accountId: '',
    jurisdiction: R2Jurisdiction.Default,
    bucket: '',
    prefix: DEFAULT_PREFIX,
    accessKeyId: '',
    secretAccessKey: '',
  }
}

function targetFromForm(form: BackupForm): OffsiteBackupTarget {
  return {
    accountId: form.accountId.trim(),
    jurisdiction: form.jurisdiction,
    bucket: form.bucket.trim(),
    prefix: form.prefix.trim(),
  }
}

function applyTargetToForm(
  form: BackupForm,
  target: OffsiteBackupTarget,
): BackupForm {
  return {
    ...form,
    accountId: target.accountId,
    jurisdiction: target.jurisdiction,
    bucket: target.bucket,
    prefix: target.prefix,
  }
}

function isTargetFormField(field: keyof BackupForm): boolean {
  return (
    field === 'accountId' ||
    field === 'jurisdiction' ||
    field === 'bucket' ||
    field === 'prefix'
  )
}

function targetsMatch(
  current: OffsiteBackupTarget | null,
  candidate: OffsiteBackupTarget,
): boolean {
  return (
    current !== null &&
    current.accountId === candidate.accountId &&
    current.jurisdiction === candidate.jurisdiction &&
    current.bucket === candidate.bucket &&
    current.prefix === candidate.prefix
  )
}

function validateForm(
  form: BackupForm,
  mode: BackupFormMode,
): BackupFormErrors {
  const errors: BackupFormErrors = {}
  const lowerHex = /^[0-9a-f]+$/
  if (mode !== BackupFormMode.ReplaceCredentials) {
    if (form.accountId.length !== 32 || !lowerHex.test(form.accountId)) {
      errors.accountId = 'Enter the 32-character lowercase R2 account ID.'
    }
    if (
      !/^[a-z0-9](?:[a-z0-9-]{1,61}[a-z0-9])$/.test(form.bucket)
    ) {
      errors.bucket =
        'Use 3–63 lowercase letters, numbers, or hyphens.'
    }
    const prefix = form.prefix
    if (
      prefix.length === 0 ||
      prefix.length > 512 ||
      prefix.startsWith('/') ||
      prefix.endsWith('/') ||
      prefix.includes('..') ||
      prefix.includes('\\') ||
      prefix.includes('?') ||
      prefix.includes('#') ||
      prefix.split('/').some((segment) => !/^[A-Za-z0-9_.-]+$/.test(segment))
    ) {
      errors.prefix =
        'Use path segments made from letters, numbers, dots, dashes, or underscores.'
    }
  }
  if (form.accessKeyId.length !== 32 || !lowerHex.test(form.accessKeyId)) {
    errors.accessKeyId =
      'Enter the 32-character lowercase R2 Access Key ID.'
  }
  if (
    form.secretAccessKey.length !== 64 ||
    !lowerHex.test(form.secretAccessKey)
  ) {
    errors.secretAccessKey =
      'Enter the 64-character lowercase R2 Secret Access Key.'
  }
  return errors
}

function clearCredentials(
  setForm: Dispatch<SetStateAction<BackupForm>>,
) {
  setForm((current) => ({
    ...current,
    accessKeyId: '',
    secretAccessKey: '',
  }))
}

function relationalLabel(phase: RelationalBackupPhase): string {
  const labels: Record<RelationalBackupPhase, string> = {
    [RelationalBackupPhase.Off]: 'Stopped',
    [RelationalBackupPhase.WaitingForCredentials]: 'Needs credentials',
    [RelationalBackupPhase.Starting]: 'Starting',
    [RelationalBackupPhase.Running]: 'Running',
    [RelationalBackupPhase.Degraded]: 'Degraded',
    [RelationalBackupPhase.Blocked]: 'Blocked',
    [RelationalBackupPhase.Unavailable]: 'Unavailable',
  }
  return labels[phase]
}

function mediaLabel(phase: MediaBackupPhase): string {
  const labels: Record<MediaBackupPhase, string> = {
    [MediaBackupPhase.Off]: 'Stopped',
    [MediaBackupPhase.WaitingForCredentials]: 'Needs credentials',
    [MediaBackupPhase.Idle]: 'Up to date',
    [MediaBackupPhase.Uploading]: 'Uploading',
    [MediaBackupPhase.RetryWait]: 'Retrying',
    [MediaBackupPhase.Blocked]: 'Blocked',
    [MediaBackupPhase.Unavailable]: 'Unavailable',
  }
  return labels[phase]
}

function checkpointLabel(phase: CheckpointBackupPhase): string {
  const labels: Record<CheckpointBackupPhase, string> = {
    [CheckpointBackupPhase.Off]: 'Stopped',
    [CheckpointBackupPhase.WaitingForMedia]: 'Waiting for media',
    [CheckpointBackupPhase.Fencing]: 'Preparing',
    [CheckpointBackupPhase.WaitingForReplica]: 'Waiting for R2',
    [CheckpointBackupPhase.Validating]: 'Validating',
    [CheckpointBackupPhase.Publishing]: 'Publishing',
    [CheckpointBackupPhase.Idle]: 'Complete',
    [CheckpointBackupPhase.Degraded]: 'Last complete copy kept',
    [CheckpointBackupPhase.Blocked]: 'Last complete copy kept',
    [CheckpointBackupPhase.Unavailable]: 'Unavailable',
  }
  return labels[phase]
}

function jurisdictionLabel(
  jurisdiction: R2Jurisdiction | undefined,
): string {
  const labels: Record<R2Jurisdiction, string> = {
    [R2Jurisdiction.Default]: 'automatic jurisdiction',
    [R2Jurisdiction.Eu]: 'EU jurisdiction',
    [R2Jurisdiction.Fedramp]: 'FedRAMP jurisdiction',
  }
  return jurisdiction ? labels[jurisdiction] : 'jurisdiction unavailable'
}

function progressMessage(phase: OffsiteBackupProgressPhase): string {
  const messages: Record<OffsiteBackupProgressPhase, string> = {
    [OffsiteBackupProgressPhase.ValidatingConfig]: 'Checking the backup settings…',
    [OffsiteBackupProgressPhase.TestingObjectStore]: 'Testing the R2 bucket…',
    [OffsiteBackupProgressPhase.TestingLitestream]: 'Testing relational backup and restore…',
    [OffsiteBackupProgressPhase.SavingConfiguration]: 'Saving the tested configuration…',
    [OffsiteBackupProgressPhase.ReconcilingMedia]: 'Preparing images for backup…',
    [OffsiteBackupProgressPhase.FencingDatabase]: 'Preparing a consistent database point…',
    [OffsiteBackupProgressPhase.WaitingForReplication]: 'Waiting for R2 to catch up…',
    [OffsiteBackupProgressPhase.PublishingCheckpoint]: 'Publishing the complete backup…',
    [OffsiteBackupProgressPhase.RestoringRelational]: 'Restoring a private test copy…',
    [OffsiteBackupProgressPhase.RestoringMedia]: 'Restoring images into the test copy…',
    [OffsiteBackupProgressPhase.ValidatingPair]: 'Checking the restored test copy…',
    [OffsiteBackupProgressPhase.Complete]: 'Backup task complete.',
    [OffsiteBackupProgressPhase.Failed]: 'Backup task failed.',
  }
  return messages[phase]
}

function abbreviate(value: string): string {
  return value.slice(0, 8)
}

function formatDate(value: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value))
}

function formatDuration(durationMs: number): string {
  if (durationMs < 1_000) {
    return `${durationMs} ms`
  }
  return `${Math.round(durationMs / 1_000)} s`
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) {
    return `${bytes} B`
  }
  if (bytes < 1_048_576) {
    return `${Math.round(bytes / 1_024)} KB`
  }
  return `${(bytes / 1_048_576).toFixed(1)} MB`
}

function errorMessage(error: unknown): string {
  if (error && typeof error === 'object') {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string' && message.trim()) {
      return message
    }
    const code = (error as { code?: unknown }).code
    if (isBackupErrorCode(code)) {
      return backupErrorMessage(code)
    }
  }
  return error instanceof Error ? error.message : String(error)
}

function isBackupErrorCode(value: unknown): value is BackupErrorCode {
  return Object.values(BackupErrorCode).includes(value as BackupErrorCode)
}

function backupErrorMessage(code: BackupErrorCode): string {
  const messages: Record<BackupErrorCode, string> = {
    [BackupErrorCode.NetworkOffline]: 'Dara could not reach Cloudflare R2.',
    [BackupErrorCode.NetworkTimeout]: 'Cloudflare R2 did not respond in time.',
    [BackupErrorCode.RateLimited]: 'Cloudflare R2 asked Dara to try again later.',
    [BackupErrorCode.ServiceUnavailable]: 'Cloudflare R2 is temporarily unavailable.',
    [BackupErrorCode.KeychainCredentialMissing]: 'Saved R2 credentials are missing.',
    [BackupErrorCode.KeychainUnavailable]: 'Dara could not use macOS Keychain.',
    [BackupErrorCode.InvalidTarget]: 'Check the R2 account, bucket, prefix, and credentials.',
    [BackupErrorCode.AuthenticationRejected]: 'Cloudflare rejected the R2 credentials.',
    [BackupErrorCode.AuthorizationRejected]: 'These credentials cannot use that bucket.',
    [BackupErrorCode.PrefixIdentityMismatch]: 'That prefix belongs to a different Dara backup.',
    [BackupErrorCode.OwnerMismatch]: 'Another Dara installation owns this backup.',
    [BackupErrorCode.ImmutableObjectConflict]: 'Remote backup data conflicts with local data.',
    [BackupErrorCode.LocalMediaMissing]: 'A local image needed for backup is missing.',
    [BackupErrorCode.LocalMediaTooLarge]: 'A local image is too large to back up.',
    [BackupErrorCode.LocalMediaHashMismatch]: 'A local image failed its integrity check.',
    [BackupErrorCode.WorkerUnavailable]: 'The backup service is unavailable. Restart Dara.',
    [BackupErrorCode.LitestreamUnavailable]: 'Dara could not start its backup helper.',
    [BackupErrorCode.LitestreamFailed]: 'The relational backup test failed.',
    [BackupErrorCode.FenceTimeout]: 'Dara could not prepare a safe database point.',
    [BackupErrorCode.ReplicaBehind]: 'Relational replication has not caught up yet.',
    [BackupErrorCode.CheckpointNotFound]: 'No complete backup checkpoint was found.',
    [BackupErrorCode.ExactTxidUnavailable]: 'That relational backup point is unavailable.',
    [BackupErrorCode.MalformedManifest]: 'Remote backup metadata is invalid.',
    [BackupErrorCode.RemoteMediaMissing]: 'A backed-up image is missing from R2.',
    [BackupErrorCode.RemoteMediaCorrupt]: 'A backed-up image failed its integrity check.',
    [BackupErrorCode.RestoreValidationFailed]: 'The restore drill did not validate.',
  }
  return messages[code]
}
