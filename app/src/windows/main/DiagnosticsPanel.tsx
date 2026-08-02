import { useCallback, useEffect, useState } from 'react'
import { DaraButton } from '../../components/DaraButton.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../components/dara-button-types.ts'
import type {
  DiagnosticsGateway,
  DiagnosticsSnapshot,
} from '../../diagnostics/index.ts'
import { DaraText } from '../../components/DaraText.tsx'
import {
  DaraTextTone,
  DaraTextVariant,
} from '../../components/dara-text-types.ts'
import { SemanticSearchPhase } from '../../review/index.ts'

const JINA_V5_TEXT_NANO_MODEL =
  'jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF'
const JINA_V5_TEXT_NANO_LABEL = 'Jina Embeddings v5 Text Nano'

export function DiagnosticsPanel({
  gateway,
}: {
  gateway: DiagnosticsGateway
}) {
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setSnapshot(await gateway.loadDiagnostics())
    } catch (loadError) {
      setError(errorMessage(loadError))
    } finally {
      setLoading(false)
    }
  }, [gateway])

  useEffect(() => {
    void reload()
  }, [reload])

  return (
    <div className="diagnostics-overview">
      <div className="diagnostics-overview-heading">
        <div>
          <DaraText as="strong" variant={DaraTextVariant.Label}>
            System status
          </DaraText>
          <DaraText
            as="span"
            tone={DaraTextTone.Muted}
            variant={DaraTextVariant.Supporting}
          >
            Current versions, local storage, and maintenance state.
          </DaraText>
        </div>
        <DaraButton
          disabled={loading}
          onClick={() => void reload()}
          size={DaraButtonSize.Mini}
          type="button"
          variant={DaraButtonVariant.Ghost}
        >
          {loading ? 'Refreshing…' : 'Refresh'}
        </DaraButton>
      </div>
      {snapshot && <DiagnosticsDetails snapshot={snapshot} />}
      {loading && !snapshot && (
        <DaraText
          as="p"
          className="diagnostics-message"
          role="status"
          tone={DaraTextTone.Muted}
          variant={DaraTextVariant.Supporting}
        >
          Loading diagnostics…
        </DaraText>
      )}
      {error && (
        <div className="diagnostics-message error" role="alert">
          <DaraText
            as="span"
            tone={DaraTextTone.Danger}
            variant={DaraTextVariant.Supporting}
          >
            {error}
          </DaraText>
          <DaraButton
            onClick={() => void reload()}
            size={DaraButtonSize.Mini}
            type="button"
          >
            Try again
          </DaraButton>
        </div>
      )}
    </div>
  )
}

function DiagnosticsDetails({
  snapshot,
}: {
  snapshot: DiagnosticsSnapshot
}) {
  const { migrationHeads, scheduler, semanticIndex } = snapshot.database
  const semanticModel = snapshot.semanticModel
  const storage = snapshot.storage
  const mediaReport = snapshot.lastMediaMaintenance
  const missingMedia =
    mediaReport?.integrity.missingReferencedBlobImageIds.length ?? 0

  return (
    <dl className="diagnostics-list">
      <DiagnosticItem
        detail={`Database main ${migrationHead(migrationHeads.main)} · media ${migrationHead(migrationHeads.media)}`}
        label="Version"
        value={`Dara ${snapshot.applicationVersion}`}
      />
      <DiagnosticItem
        detail={`Desired retention ${Math.round(scheduler.desiredRetention * 100)}%`}
        label="Scheduling"
        value={`${scheduler.algorithm} ${scheduler.algorithmVersion} · ${scheduler.schedulerLibrary} ${scheduler.libraryVersion}`}
      />
      <DiagnosticItem
        detail={semanticSearchDetail(semanticIndex, semanticModel)}
        label="Semantic search"
        value={semanticPhaseLabel(semanticModel.phase)}
      />
      <DiagnosticItem
        detail={`Database ${formatBytes(storage.relationalDatabaseBytes)} · media ${formatBytes(storage.mediaDatabaseBytes)} · model ${formatBytes(storage.modelBytes)} · snapshots ${formatBytes(storage.snapshotsBytes)} · logs ${formatBytes(storage.logsBytes)}`}
        label="Storage"
        value={formatBytes(
          storage.relationalDatabaseBytes +
            storage.mediaDatabaseBytes +
            storage.modelBytes +
            storage.snapshotsBytes +
            storage.logsBytes,
        )}
      />
      <DiagnosticItem
        detail={
          snapshot.latestSnapshot
            ? `Created by Dara ${snapshot.latestSnapshot.applicationVersion}`
            : 'No finalized local snapshot found'
        }
        label="Latest snapshot"
        value={
          snapshot.latestSnapshot
            ? formatDateTime(snapshot.latestSnapshot.createdAt)
            : 'No local snapshot'
        }
      />
      <DiagnosticItem
        detail={
          mediaReport
            ? missingMedia === 0
              ? 'No missing referenced media found'
              : `${missingMedia.toLocaleString()} missing referenced ${missingMedia === 1 ? 'blob' : 'blobs'} found`
            : 'No maintenance result is available yet'
        }
        label="Media check"
        value={
          mediaReport
            ? formatDateTime(mediaReport.inspectedAt)
            : 'Not checked'
        }
        warning={missingMedia > 0}
      />
    </dl>
  )
}

function DiagnosticItem({
  detail,
  label,
  value,
  warning = false,
}: {
  detail: string
  label: string
  value: string
  warning?: boolean
}) {
  return (
    <div className={`diagnostics-item${warning ? ' warning' : ''}`}>
      <dt>
        <DaraText
          as="span"
          tone={DaraTextTone.Muted}
          variant={DaraTextVariant.Caption}
        >
          {label}
        </DaraText>
      </dt>
      <dd>
        <DaraText
          as="strong"
          tone={warning ? DaraTextTone.Warning : DaraTextTone.Default}
          variant={DaraTextVariant.Body}
        >
          {value}
        </DaraText>
        <DaraText
          as="span"
          tone={warning ? DaraTextTone.Warning : DaraTextTone.Muted}
          variant={DaraTextVariant.Caption}
        >
          {detail}
        </DaraText>
      </dd>
    </div>
  )
}

function migrationHead(head: number | null): string {
  return head === null ? 'none' : head.toString()
}

function semanticPhaseLabel(
  phase: DiagnosticsSnapshot['semanticModel']['phase'],
): string {
  switch (phase) {
    case SemanticSearchPhase.Downloading:
      return 'Downloading'
    case SemanticSearchPhase.Verifying:
      return 'Verifying'
    case SemanticSearchPhase.Starting:
      return 'Starting'
    case SemanticSearchPhase.Indexing:
      return 'Indexing'
    case SemanticSearchPhase.Ready:
      return 'Ready'
    case SemanticSearchPhase.Unavailable:
      return 'Unavailable'
    case SemanticSearchPhase.Failed:
      return 'Needs attention'
  }
}

function semanticSearchDetail(
  index: DiagnosticsSnapshot['database']['semanticIndex'],
  model: DiagnosticsSnapshot['semanticModel'],
): string {
  const modelName = semanticModelLabel(model.modelName)
  const indexProgress = `${index.indexedDocuments.toLocaleString()} of ${index.totalDocuments.toLocaleString()} cards indexed`

  if (model.lastError) {
    return `${modelName} · ${model.lastError}`
  }

  switch (model.phase) {
    case SemanticSearchPhase.Downloading:
      return `${modelName} · ${formatBytes(model.downloadedBytes)} of ${formatBytes(model.expectedBytes)} downloaded`
    case SemanticSearchPhase.Verifying:
      return `${modelName} · Verifying the downloaded model`
    case SemanticSearchPhase.Starting:
      return `${modelName} · Starting the local search service`
    case SemanticSearchPhase.Indexing:
      return `${modelName} · ${indexProgress}`
    case SemanticSearchPhase.Ready:
      return `${modelName} · ${indexProgress} · ${formatBytes(model.expectedBytes)} installed`
    case SemanticSearchPhase.Unavailable:
      return `${modelName} · Keyword search still works`
    case SemanticSearchPhase.Failed:
      return `${modelName} · Semantic search needs attention`
  }
}

function semanticModelLabel(modelName: string): string {
  return modelName === JINA_V5_TEXT_NANO_MODEL
    ? JINA_V5_TEXT_NANO_LABEL
    : modelName
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`
  }
  const units = ['KB', 'MB', 'GB', 'TB'] as const
  let value = bytes / 1024
  let unit: (typeof units)[number] = units[0]
  for (const nextUnit of units.slice(1)) {
    if (value < 1024) {
      break
    }
    value /= 1024
    unit = nextUnit
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${unit}`
}

function formatDateTime(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(timestamp)
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
