import { listen } from '@tauri-apps/api/event'
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { DaraPercentageControl } from '../../components/DaraPercentageControl.tsx'
import { DaraButton } from '../../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../components/dara-button-types.ts'
import { DaraSelect } from '../../components/DaraSelect.tsx'
import { DaraShortcutRecorder } from '../../components/DaraShortcutRecorder.tsx'
import { DaraToggle } from '../../components/DaraToggle.tsx'
import {
  tauriDiagnosticsGateway,
  type DiagnosticsGateway,
} from '../../diagnostics/index.ts'
import {
  changeDesiredRetention,
  checkSchedulingData,
  repairSchedulingData,
  tauriSchedulerMaintenanceGateway,
  type SchedulerMaintenanceGateway,
} from '../../scheduling/index.ts'
import {
  Appearance,
  DaraCommand,
  DEFAULT_KEYBOARD_BINDINGS,
  DEFAULT_ZOOM_PERCENT,
  MAX_ZOOM_PERCENT,
  MIN_ZOOM_PERCENT,
  ZOOM_STEP_PERCENT,
  tauriSettingsGateway,
  type KeyboardBinding,
  type SettingsGateway,
  type SettingsSnapshot,
} from '../../settings/index.ts'
import { DaraEvent } from '../../lib/tauri-contracts.ts'
import { ConfirmationDialog } from './ConfirmationDialog.tsx'
import { DiagnosticsPanel } from './DiagnosticsPanel.tsx'
import { OffsiteBackupSection } from './OffsiteBackupSection.tsx'
import {
  tauriOffsiteBackupGateway,
  type OffsiteBackupGateway,
} from '../../backup/index.ts'

const MIN_RETENTION_PERCENT = 70
const MAX_RETENTION_PERCENT = 99
const DEFAULT_RETENTION_PERCENT = 90

const SettingsMutation = {
  Appearance: 'APPEARANCE',
  AutomaticUpdateChecks: 'AUTOMATIC_UPDATE_CHECKS',
  KeyboardBindings: 'KEYBOARD_BINDINGS',
  LaunchAtLogin: 'LAUNCH_AT_LOGIN',
  Zoom: 'ZOOM',
} as const

type SettingsMutation =
  (typeof SettingsMutation)[keyof typeof SettingsMutation]

const ConfirmationKind = {
  Retention: 'RETENTION',
  SchedulingRepair: 'SCHEDULING_REPAIR',
} as const

type ConfirmationKind =
  (typeof ConfirmationKind)[keyof typeof ConfirmationKind]

const SchedulingTask = {
  Check: 'CHECK',
  Repair: 'REPAIR',
  Retention: 'RETENTION',
} as const

type SchedulingTask = (typeof SchedulingTask)[keyof typeof SchedulingTask]

const appearanceOptions = [
  { label: 'System', value: Appearance.System },
  { label: 'Light', value: Appearance.Light },
  { label: 'Dark', value: Appearance.Dark },
] as const

interface SettingsProps {
  backupGateway?: OffsiteBackupGateway
  diagnosticsGateway?: DiagnosticsGateway
  navigationToken: number
  onBusyChange: (busy: boolean) => void
  onSchedulingChanged: () => void
  reviewSaveInFlight: boolean
  schedulerGateway?: SchedulerMaintenanceGateway
  settingsGateway?: SettingsGateway
}

interface RecalculationProgress {
  completedCards: number
  finalizing: boolean
  totalCards: number
}

export function Settings({
  backupGateway = tauriOffsiteBackupGateway,
  diagnosticsGateway = tauriDiagnosticsGateway,
  navigationToken,
  onBusyChange,
  onSchedulingChanged,
  reviewSaveInFlight,
  schedulerGateway = tauriSchedulerMaintenanceGateway,
  settingsGateway = tauriSettingsGateway,
}: SettingsProps) {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null)
  const [loadingError, setLoadingError] = useState<string | null>(null)
  const [mutation, setMutation] = useState<SettingsMutation | null>(null)
  const [mutationError, setMutationError] = useState<string | null>(null)
  const [shortcutResetToken, setShortcutResetToken] = useState(0)
  const [retentionPercent, setRetentionPercent] = useState(
    DEFAULT_RETENTION_PERCENT,
  )
  const [retentionDirty, setRetentionDirty] = useState(false)
  const [confirmation, setConfirmation] = useState<ConfirmationKind | null>(null)
  const [schedulingTask, setSchedulingTask] = useState<SchedulingTask | null>(null)
  const [backupBusy, setBackupBusy] = useState(false)
  const [recalculationProgress, setRecalculationProgress] =
    useState<RecalculationProgress | null>(null)
  const [schedulingNotice, setSchedulingNotice] = useState<string | null>(null)
  const [schedulingError, setSchedulingError] = useState<string | null>(null)
  const abortControllerRef = useRef<AbortController | null>(null)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const retentionButtonRef = useRef<HTMLButtonElement>(null)
  const repairButtonRef = useRef<HTMLButtonElement>(null)
  const settingsReady = snapshot !== null

  useEffect(() => {
    headingRef.current?.focus()
  }, [navigationToken, settingsReady])

  const applySnapshot = useCallback(
    (next: SettingsSnapshot, forceRetention = false) => {
      setSnapshot(next)
      setLoadingError(null)
      if (forceRetention || !retentionDirty) {
        setRetentionPercent(Math.round(next.desiredRetention * 100))
        setRetentionDirty(false)
      }
    },
    [retentionDirty],
  )

  const reload = useCallback(async () => {
    try {
      applySnapshot(await settingsGateway.loadSettings())
    } catch (error) {
      setLoadingError(errorMessage(error))
    }
  }, [applySnapshot, settingsGateway])

  useEffect(() => {
    void reload()
  }, [reload])

  useEffect(() => {
    let disposed = false
    let stopListening: (() => void) | undefined
    void listen<SettingsSnapshot>(DaraEvent.SettingsChanged, (event) => {
      if (!disposed) {
        applySnapshot(event.payload)
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
  }, [applySnapshot])

  useEffect(
    () => () => {
      abortControllerRef.current?.abort()
      onBusyChange(false)
    },
    [onBusyChange],
  )

  const schedulingBusy = schedulingTask !== null
  const settingsBusy = backupBusy || schedulingBusy

  useEffect(() => {
    onBusyChange(settingsBusy)
  }, [onBusyChange, settingsBusy])

  const updateSetting = async (
    kind: SettingsMutation,
    operation: (current: SettingsSnapshot) => Promise<SettingsSnapshot>,
  ) => {
    if (!snapshot || mutation !== null) {
      return
    }
    setMutation(kind)
    setMutationError(null)
    try {
      applySnapshot(await operation(snapshot))
    } catch (error) {
      setMutationError(errorMessage(error))
      await reload()
    } finally {
      setMutation(null)
    }
  }

  const setBinding = (command: DaraCommand, accelerator: string) => {
    if (!snapshot) {
      return
    }
    const candidate = snapshot.keyboardBindings.map((binding) =>
      binding.command === command ? { ...binding, accelerator } : binding,
    )
    void updateSetting(SettingsMutation.KeyboardBindings, (current) =>
      settingsGateway.setKeyboardBindings(current.revision, candidate),
    )
  }

  const resetBindings = () => {
    setShortcutResetToken((value) => value + 1)
    void updateSetting(SettingsMutation.KeyboardBindings, (current) =>
      settingsGateway.setKeyboardBindings(
        current.revision,
        DEFAULT_KEYBOARD_BINDINGS.map((binding) => ({ ...binding })),
      ),
    )
  }

  const runSchedulingCheck = async () => {
    setSchedulingTask(SchedulingTask.Check)
    setSchedulingNotice(null)
    setSchedulingError(null)
    try {
      const report = await checkSchedulingData(schedulerGateway)
      setSchedulingNotice(
        report.differingCards === 0
          ? `All ${report.evaluatedCards} reviewed cards are scheduled correctly.`
          : `${report.differingCards} of ${report.evaluatedCards} reviewed cards need repair.`,
      )
    } catch (error) {
      setSchedulingError(errorMessage(error))
    } finally {
      setSchedulingTask(null)
    }
  }

  const runSchedulingRepair = async () => {
    setConfirmation(null)
    setSchedulingTask(SchedulingTask.Repair)
    setSchedulingNotice(null)
    setSchedulingError(null)
    try {
      const report = await repairSchedulingData(schedulerGateway)
      onSchedulingChanged()
      setSchedulingNotice(
        report.installedCards === 0
          ? 'Scheduling data was already correct.'
          : `Repaired ${report.installedCards} reviewed ${report.installedCards === 1 ? 'card' : 'cards'}. Review history was unchanged.`,
      )
    } catch (error) {
      setSchedulingError(errorMessage(error))
    } finally {
      setSchedulingTask(null)
    }
  }

  const runRetentionUpdate = async () => {
    if (!snapshot || reviewSaveInFlight) {
      return
    }
    setSchedulingTask(SchedulingTask.Retention)
    setSchedulingNotice(null)
    setSchedulingError(null)
    setRecalculationProgress({
      completedCards: 0,
      finalizing: false,
      totalCards: 0,
    })
    const abortController = new AbortController()
    abortControllerRef.current = abortController
    try {
      const report = await changeDesiredRetention(
        retentionPercent / 100,
        schedulerGateway,
        {
          onBeforeInstall: () => {
            setRecalculationProgress((current) =>
              current ? { ...current, finalizing: true } : current,
            )
          },
          onProgress: ({ completedCards, totalCards }) => {
            setRecalculationProgress({
              completedCards,
              finalizing: false,
              totalCards,
            })
          },
          signal: abortController.signal,
        },
      )
      const next = await settingsGateway.loadSettings()
      applySnapshot(next, true)
      setConfirmation(null)
      onSchedulingChanged()
      setSchedulingNotice(
        `Desired retention is now ${retentionPercent}%. Recalculated ${report.installedCards} reviewed ${report.installedCards === 1 ? 'card' : 'cards'}.`,
      )
    } catch (error) {
      if (abortController.signal.aborted) {
        setConfirmation(null)
        setSchedulingNotice('Schedule update cancelled. Nothing was changed.')
      } else {
        setSchedulingError(errorMessage(error))
      }
    } finally {
      abortControllerRef.current = null
      setRecalculationProgress(null)
      setSchedulingTask(null)
    }
  }

  if (!snapshot) {
    return (
      <section aria-labelledby="settings-heading" className="settings-screen settings-loading">
        <h1 id="settings-heading">Settings</h1>
        {loadingError ? (
          <div className="settings-load-error" role="alert">
            <span>{loadingError}</span>
            <DaraButton onClick={() => void reload()} type="button">
              Try again
            </DaraButton>
          </div>
        ) : (
          <p>Loading settings…</p>
        )}
      </section>
    )
  }

  const controlsDisabled = mutation !== null || schedulingBusy || backupBusy
  const activeRetentionPercent = Math.round(snapshot.desiredRetention * 100)
  const retentionChanged = retentionPercent !== activeRetentionPercent

  return (
    <section aria-labelledby="settings-heading" className="settings-screen">
      <h1 className="visually-hidden" id="settings-heading" ref={headingRef} tabIndex={-1}>
        Settings
      </h1>

      {mutationError && <div className="settings-banner error" role="alert">{mutationError}</div>}

      <SettingSection description="How Dara starts and stays close at hand." title="General">
        <SettingRow
          control={
            <DaraToggle
              checked={snapshot.launchAtLogin}
              disabled={controlsDisabled}
              label="Launch at login"
              onChange={(enabled) => {
                void updateSetting(SettingsMutation.LaunchAtLogin, () =>
                  settingsGateway.setLaunchAtLogin(enabled),
                )
              }}
            />
          }
          description="Keep Dara available in the menu bar after you sign in."
          label="Launch at login"
        />
        {snapshot.launchAtLoginError && (
          <p className="setting-inline-error" role="alert">{snapshot.launchAtLoginError}</p>
        )}
        <SettingRow
          control={
            <DaraToggle
              checked={snapshot.automaticUpdateChecksEnabled}
              disabled={controlsDisabled}
              label="Automatically check for updates"
              onChange={(enabled) => {
                void updateSetting(SettingsMutation.AutomaticUpdateChecks, (current) =>
                  settingsGateway.setAutomaticUpdateChecks(current.revision, enabled),
                )
              }}
            />
          }
          description="When on, Dara asks GitHub for the latest release shortly after launch and every 6 hours. You can still use Check for Updates… when this is off."
          label="Automatically check for updates"
        />
      </SettingSection>

      <SettingSection description="Global shortcuts work even while another app is active." title="Shortcuts">
        <SettingRow
          control={
            <DaraShortcutRecorder
              accelerator={acceleratorFor(snapshot.keyboardBindings, DaraCommand.QuickAdd)}
              disabled={controlsDisabled}
              label="Quick Add shortcut"
              onCapture={(accelerator) => setBinding(DaraCommand.QuickAdd, accelerator)}
              resetToken={shortcutResetToken}
            />
          }
          description="Open the lightweight capture window from anywhere."
          label="Quick Add"
        />
        <SettingRow
          control={
            <DaraShortcutRecorder
              accelerator={acceleratorFor(snapshot.keyboardBindings, DaraCommand.Home)}
              disabled={controlsDisabled}
              label="Home shortcut"
              onCapture={(accelerator) => setBinding(DaraCommand.Home, accelerator)}
              resetToken={shortcutResetToken}
            />
          }
          description="Bring Dara forward and show Home."
          label="Home"
        />
        {snapshot.shortcutErrors.map((error) => (
          <p className="setting-inline-error" key={error} role="alert">{error}</p>
        ))}
        <div className="setting-section-footer">
          <DaraButton
            disabled={controlsDisabled}
            onClick={resetBindings}
            type="button"
            variant={DaraButtonVariant.Ghost}
          >
            Reset shortcuts
          </DaraButton>
        </div>
      </SettingSection>

      <SettingSection description="Choose how often Dara asks you to revisit material." title="Review">
        <div className="retention-setting">
          <div className="retention-heading">
            <div>
              <strong>Desired retention</strong>
              <span id="retention-description">
                Higher values mean more reviews and less forgetting. Lower values reduce workload.
              </span>
            </div>
            {retentionChanged && <span className="unsaved-pill">Not applied</span>}
          </div>
          <DaraPercentageControl
            describedBy="retention-description"
            disabled={controlsDisabled}
            label="Desired retention"
            max={MAX_RETENTION_PERCENT}
            min={MIN_RETENTION_PERCENT}
            onChange={(value) => {
              setRetentionPercent(value)
              setRetentionDirty(value !== activeRetentionPercent)
              setSchedulingNotice(null)
              setSchedulingError(null)
            }}
            value={retentionPercent}
          />
          <div className="retention-actions">
            <DaraButton
              disabled={!retentionChanged || controlsDisabled || reviewSaveInFlight}
              onClick={() => setConfirmation(ConfirmationKind.Retention)}
              ref={retentionButtonRef}
              type="button"
              variant={DaraButtonVariant.Accent}
            >
              Update schedule
            </DaraButton>
            {retentionChanged && retentionPercent !== DEFAULT_RETENTION_PERCENT && (
              <DaraButton
                className="quiet-button"
                disabled={controlsDisabled}
                onClick={() => {
                  setRetentionPercent(DEFAULT_RETENTION_PERCENT)
                  setRetentionDirty(DEFAULT_RETENTION_PERCENT !== activeRetentionPercent)
                }}
                type="button"
                variant={DaraButtonVariant.Ghost}
              >
                Restore 90%
              </DaraButton>
            )}
          </div>
          {reviewSaveInFlight && (
            <p className="setting-note">Finish saving the current review before updating the schedule.</p>
          )}
        </div>
      </SettingSection>

      <SettingSection description="Changes apply to both Dara windows." title="Appearance">
        <SettingRow
          control={
            <DaraSelect
              ariaLabel="Appearance"
              disabled={controlsDisabled}
              menuHeight={122}
              menuWidth={150}
              onSelect={(appearance) => {
                void updateSetting(SettingsMutation.Appearance, (current) =>
                  settingsGateway.setAppearance(current.revision, appearance),
                )
              }}
              options={appearanceOptions}
              value={snapshot.appearance}
            />
          }
          description="Follow macOS, or keep Dara consistently light or dark."
          label="Theme"
        />
        <SettingRow
          control={
            <div className="zoom-stepper">
              <DaraButton
                aria-label="Zoom out"
                disabled={controlsDisabled || snapshot.zoomPercent <= MIN_ZOOM_PERCENT}
                onClick={() => {
                  void updateSetting(SettingsMutation.Zoom, (current) =>
                    settingsGateway.setZoomPercent(
                      current.revision,
                      current.zoomPercent - ZOOM_STEP_PERCENT,
                    ),
                  )
                }}
                size={DaraButtonSize.Icon}
                type="button"
              >
                −
              </DaraButton>
              <output aria-live="polite">{snapshot.zoomPercent}%</output>
              <DaraButton
                aria-label="Zoom in"
                disabled={controlsDisabled || snapshot.zoomPercent >= MAX_ZOOM_PERCENT}
                onClick={() => {
                  void updateSetting(SettingsMutation.Zoom, (current) =>
                    settingsGateway.setZoomPercent(
                      current.revision,
                      current.zoomPercent + ZOOM_STEP_PERCENT,
                    ),
                  )
                }}
                size={DaraButtonSize.Icon}
                type="button"
              >
                +
              </DaraButton>
              {snapshot.zoomPercent !== DEFAULT_ZOOM_PERCENT && (
                <DaraButton
                  className="zoom-reset"
                  disabled={controlsDisabled}
                  onClick={() => {
                    void updateSetting(SettingsMutation.Zoom, (current) =>
                      settingsGateway.setZoomPercent(current.revision, DEFAULT_ZOOM_PERCENT),
                    )
                  }}
                  size={DaraButtonSize.Mini}
                  type="button"
                  variant={DaraButtonVariant.Ghost}
                >
                  Reset
                </DaraButton>
              )}
            </div>
          }
          description="Adjust text and controls from 50% to 200%."
          label="Zoom"
        />
      </SettingSection>

      <OffsiteBackupSection
        disabled={controlsDisabled}
        gateway={backupGateway}
        onBusyChange={setBackupBusy}
      />

      <SettingSection
        className="diagnostics-section"
        description="A read-only summary of Dara’s local data and services."
        title="Data & diagnostics"
      >
        <DiagnosticsPanel gateway={diagnosticsGateway} />
        <div className="diagnostic-action">
          <div>
            <strong>Scheduling data</strong>
            <span>Check that every reviewed card matches its history.</span>
          </div>
          <div>
            <DaraButton
              disabled={controlsDisabled}
              onClick={() => void runSchedulingCheck()}
              type="button"
            >
              {schedulingTask === SchedulingTask.Check ? 'Checking…' : 'Check'}
            </DaraButton>
            <DaraButton
              disabled={controlsDisabled}
              onClick={() => setConfirmation(ConfirmationKind.SchedulingRepair)}
              ref={repairButtonRef}
              type="button"
            >
              Repair data
            </DaraButton>
          </div>
        </div>
        {schedulingNotice && <p className="scheduling-result" role="status">{schedulingNotice}</p>}
        {schedulingError && confirmation === null && (
          <p className="scheduling-result error" role="alert">{schedulingError}</p>
        )}
      </SettingSection>

      {confirmation === ConfirmationKind.Retention && (
        <ConfirmationDialog
          allowCancelWhileBusy={Boolean(
            recalculationProgress && !recalculationProgress.finalizing,
          )}
          busy={schedulingTask === SchedulingTask.Retention}
          confirmLabel={
            schedulingTask === SchedulingTask.Retention
              ? 'Updating schedule…'
              : `Update to ${retentionPercent}%`
          }
          onCancel={() => {
            if (recalculationProgress && !recalculationProgress.finalizing) {
              abortControllerRef.current?.abort()
            } else if (schedulingTask === null) {
              setConfirmation(null)
              requestAnimationFrame(() => retentionButtonRef.current?.focus())
            }
          }}
          onConfirm={() => void runRetentionUpdate()}
          title="Recalculate reviewed cards?"
        >
          <p>
            Dara will rebuild every reviewed card’s schedule using {retentionPercent}% desired
            retention. Your cards and review history will not change.
          </p>
          <p>
            The current schedule stays active unless the entire recalculation succeeds.
          </p>
          {recalculationProgress && (
            <div className="recalculation-progress" role="status">
              <progress
                max={Math.max(1, recalculationProgress.totalCards)}
                value={recalculationProgress.completedCards}
              />
              <span>
                {recalculationProgress.finalizing
                  ? 'Saving the new schedule…'
                  : recalculationProgress.totalCards === 0
                    ? 'Preparing reviewed cards…'
                    : `${recalculationProgress.completedCards} of ${recalculationProgress.totalCards} cards`}
              </span>
            </div>
          )}
          {schedulingError && <p className="dialog-error" role="alert">{schedulingError}</p>}
        </ConfirmationDialog>
      )}

      {confirmation === ConfirmationKind.SchedulingRepair && (
        <ConfirmationDialog
          busy={schedulingTask === SchedulingTask.Repair}
          confirmLabel={schedulingTask === SchedulingTask.Repair ? 'Repairing…' : 'Repair data'}
          onCancel={() => {
            if (schedulingTask === null) {
              setConfirmation(null)
              requestAnimationFrame(() => repairButtonRef.current?.focus())
            }
          }}
          onConfirm={() => void runSchedulingRepair()}
          title="Repair scheduling data?"
        >
          <p>
            Dara will replace only scheduling caches that differ from their review history.
            Cards and review history remain untouched.
          </p>
        </ConfirmationDialog>
      )}
    </section>
  )
}

function SettingSection({
  children,
  className,
  description,
  title,
}: {
  children: ReactNode
  className?: string
  description: string
  title: string
}) {
  return (
    <section className={`setting-section${className ? ` ${className}` : ''}`}>
      <div className="setting-section-heading">
        <h2>{title}</h2>
        <p>{description}</p>
      </div>
      <div className="setting-section-body">{children}</div>
    </section>
  )
}

function SettingRow({
  control,
  description,
  label,
}: {
  control: ReactNode
  description: string
  label: string
}) {
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{description}</span>
      </div>
      {control}
    </div>
  )
}

function acceleratorFor(
  bindings: KeyboardBinding[],
  command: DaraCommand,
): string {
  return (
    bindings.find((binding) => binding.command === command)?.accelerator ?? ''
  )
}

function errorMessage(error: unknown): string {
  if (error && typeof error === 'object') {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string' && message.trim()) {
      return message
    }
  }
  return error instanceof Error ? error.message : String(error)
}
