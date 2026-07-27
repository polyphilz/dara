#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
APP_ROOT="$ROOT/app"
PIN="$APP_ROOT/src-tauri/resources/sidecars/litestream-v1.json"
ENV_FILE=${DARA_LITESTREAM_ENV_FILE:-"$APP_ROOT/.env.local"}
VERIFICATION_ROOT="$APP_ROOT/.data/litestream-protocol-tests"
DOWNLOAD="$VERIFICATION_ROOT/downloads/$(jq -er '.upstream.asset.name' "$PIN")"

for command in curl jq shasum sqlite3 uuidgen; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

test -r "$ENV_FILE" || {
  echo "Litestream development environment file not found: $ENV_FILE" >&2
  echo "copy app/.env.example to app/.env.local and fill in bucket-scoped R2 credentials" >&2
  exit 1
}

set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

for variable in \
  DARA_LITESTREAM_R2_ACCOUNT_ID \
  DARA_LITESTREAM_R2_JURISDICTION \
  DARA_LITESTREAM_R2_BUCKET \
  DARA_LITESTREAM_R2_PREFIX \
  DARA_LITESTREAM_R2_ACCESS_KEY_ID \
  DARA_LITESTREAM_R2_SECRET_ACCESS_KEY
do
  eval "value=\${$variable:-}"
  test -n "$value" || {
    echo "missing $variable in $ENV_FILE" >&2
    exit 1
  }
done

case "$DARA_LITESTREAM_R2_JURISDICTION" in
  DEFAULT)
    ENDPOINT="https://${DARA_LITESTREAM_R2_ACCOUNT_ID}.r2.cloudflarestorage.com"
    ;;
  EU)
    ENDPOINT="https://${DARA_LITESTREAM_R2_ACCOUNT_ID}.eu.r2.cloudflarestorage.com"
    ;;
  FEDRAMP)
    ENDPOINT="https://${DARA_LITESTREAM_R2_ACCOUNT_ID}.fedramp.r2.cloudflarestorage.com"
    ;;
  *)
    echo "unsupported R2 jurisdiction: $DARA_LITESTREAM_R2_JURISDICTION" >&2
    exit 1
    ;;
esac

mkdir -p "$VERIFICATION_ROOT/downloads"
if test ! -f "$DOWNLOAD"; then
  curl -fL --retry 3 --connect-timeout 15 \
    "$(jq -er '.upstream.asset.url' "$PIN")" \
    -o "$DOWNLOAD"
fi

EXPECTED_ARCHIVE_SHA256=$(jq -er '.upstream.asset.sha256' "$PIN")
ACTUAL_ARCHIVE_SHA256=$(shasum -a 256 "$DOWNLOAD" | awk '{print $1}')
test "$ACTUAL_ARCHIVE_SHA256" = "$EXPECTED_ARCHIVE_SHA256" || {
  echo "cached Litestream archive checksum mismatch" >&2
  exit 1
}

"$ROOT/scripts/stage-litestream-sidecar.sh" "$DOWNLOAD" >/dev/null
BINARY="$APP_ROOT/src-tauri/resources/release/bin/litestream"

RUN_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
SHORT_ID=${RUN_ID%%-*}
RUN_ROOT="$VERIFICATION_ROOT/runs/$RUN_ID"
RUNTIME_ROOT="$VERIFICATION_ROOT/rt/$SHORT_ID"
DATABASE="$RUN_ROOT/dara.sqlite3"
CONFIG="$RUN_ROOT/litestream.yml"
SOCKET="$RUNTIME_ROOT/ls.sock"
REMOTE_PATH="${DARA_LITESTREAM_R2_PREFIX%/}/runs/$RUN_ID/dara.sqlite3"
LOG="$RUN_ROOT/litestream.log"
DAEMON_PID=

mkdir -p "$RUN_ROOT/restores" "$RUNTIME_ROOT"
chmod 700 "$RUN_ROOT" "$RUN_ROOT/restores" "$RUNTIME_ROOT"

jq -n \
  --arg socket "$SOCKET" \
  --arg database "$DATABASE" \
  --arg bucket "$DARA_LITESTREAM_R2_BUCKET" \
  --arg replicaPath "$REMOTE_PATH" \
  --arg endpoint "$ENDPOINT" \
  '{
    logging: {level: "info", type: "json", stderr: true},
    socket: {enabled: true, path: $socket},
    "sync-interval": "5s",
    "verify-compaction": true,
    "auto-recover": false,
    "l0-retention": "720h",
    "l0-retention-check-interval": "1m",
    "shutdown-sync-timeout": "30s",
    "shutdown-sync-interval": "500ms",
    snapshot: {interval: "6h", retention: "720h"},
    validation: {interval: "6h"},
    dbs: [{
      path: $database,
      "monitor-interval": "1s",
      "checkpoint-interval": "1m",
      replica: {
        type: "s3",
        bucket: $bucket,
        path: $replicaPath,
        endpoint: $endpoint,
        region: "auto",
        "access-key-id": "${DARA_LITESTREAM_R2_ACCESS_KEY_ID}",
        "secret-access-key": "${DARA_LITESTREAM_R2_SECRET_ACCESS_KEY}",
        "force-path-style": false,
        "sync-interval": "5s"
      }
    }]
  }' >"$CONFIG"
chmod 600 "$CONFIG"

stop_daemon() {
  if test -n "$DAEMON_PID" && kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill -TERM "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
}
trap stop_daemon EXIT INT TERM

sqlite3 "$DATABASE" \
  "PRAGMA journal_mode=WAL;
   PRAGMA synchronous=FULL;
   CREATE TABLE fence_events(sequence INTEGER PRIMARY KEY, marker TEXT NOT NULL UNIQUE);
   INSERT INTO fence_events(marker) VALUES('baseline');" \
  >/dev/null

"$BINARY" replicate -config "$CONFIG" >"$LOG" 2>&1 &
DAEMON_PID=$!

socket_deadline=$((SECONDS + 15))
while test ! -S "$SOCKET"; do
  kill -0 "$DAEMON_PID" 2>/dev/null || {
    echo "Litestream exited before creating its control socket; see $LOG" >&2
    exit 1
  }
  test "$SECONDS" -lt "$socket_deadline" || {
    echo "timed out waiting for Litestream control socket; see $LOG" >&2
    exit 1
  }
  sleep 1
done
test "$(stat -f '%Lp' "$SOCKET")" = 600 || {
  echo "Litestream control socket is not mode 0600" >&2
  exit 1
}

sqlite3 "$DATABASE" "INSERT INTO fence_events(marker) VALUES('inside-fence');"
LOCAL_SYNC=$("$BINARY" sync -json -socket "$SOCKET" "$DATABASE")
LOCAL_TXID=$(printf '%s' "$LOCAL_SYNC" | jq -er '.txid')
FENCED_TXID=$(printf '%016x' "$LOCAL_TXID")

sqlite3 "$DATABASE" "INSERT INTO fence_events(marker) VALUES('after-fence');"
REMOTE_SYNC=$("$BINARY" sync -wait -timeout 60 -json -socket "$SOCKET" "$DATABASE")
REPLICA_TXID=$(printf '%s' "$REMOTE_SYNC" | jq -er '.replica_txid')
test "$REPLICA_TXID" -ge "$LOCAL_TXID"

"$BINARY" restore \
  -config "$CONFIG" \
  -txid "$FENCED_TXID" \
  -dry-run \
  -json \
  -o "$RUN_ROOT/restores/pre-compaction.sqlite3" \
  "$DATABASE" \
  >"$RUN_ROOT/restores/pre-compaction-plan.json"

"$BINARY" restore \
  -config "$CONFIG" \
  -txid "$FENCED_TXID" \
  -json \
  -integrity-check full \
  -o "$RUN_ROOT/restores/pre-compaction.sqlite3" \
  "$DATABASE" \
  >"$RUN_ROOT/restores/pre-compaction-result.json"

test "$(sqlite3 "$RUN_ROOT/restores/pre-compaction.sqlite3" \
  "SELECT COUNT(*) FROM fence_events WHERE marker='inside-fence'")" = 1
test "$(sqlite3 "$RUN_ROOT/restores/pre-compaction.sqlite3" \
  "SELECT COUNT(*) FROM fence_events WHERE marker='after-fence'")" = 0

compaction_deadline=$((SECONDS + 60))
while :; do
  LTX_JSON=$("$BINARY" ltx -config "$CONFIG" -level all -json "$DATABASE")
  if printf '%s' "$LTX_JSON" |
    jq -e --arg txid "$FENCED_TXID" \
      'any(.[]; .level == 1 and .min_txid <= $txid and .max_txid > $txid)' \
      >/dev/null
  then
    break
  fi
  test "$SECONDS" -lt "$compaction_deadline" || {
    echo "timed out waiting for ordinary L1 compaction" >&2
    exit 1
  }
  sleep 2
done

"$BINARY" restore \
  -config "$CONFIG" \
  -txid "$FENCED_TXID" \
  -json \
  -integrity-check full \
  -o "$RUN_ROOT/restores/post-compaction.sqlite3" \
  "$DATABASE" \
  >"$RUN_ROOT/restores/post-compaction-result.json"

test "$(sqlite3 "$RUN_ROOT/restores/post-compaction.sqlite3" \
  "SELECT COUNT(*) FROM fence_events WHERE marker='inside-fence'")" = 1
test "$(sqlite3 "$RUN_ROOT/restores/post-compaction.sqlite3" \
  "SELECT COUNT(*) FROM fence_events WHERE marker='after-fence'")" = 0

sqlite3 "$DATABASE" "INSERT INTO fence_events(marker) VALUES('shutdown-only-sync');"
kill -TERM "$DAEMON_PID"
wait "$DAEMON_PID"
DAEMON_PID=

"$BINARY" restore \
  -config "$CONFIG" \
  -json \
  -integrity-check full \
  -o "$RUN_ROOT/restores/shutdown-latest.sqlite3" \
  "$DATABASE" \
  >"$RUN_ROOT/restores/shutdown-result.json"

test "$(sqlite3 "$RUN_ROOT/restores/shutdown-latest.sqlite3" \
  "SELECT COUNT(*) FROM fence_events WHERE marker='shutdown-only-sync'")" = 1

jq -n \
  --arg runId "$RUN_ID" \
  --arg remotePath "$REMOTE_PATH" \
  --arg fencedTxid "$FENCED_TXID" \
  --argjson remoteTxid "$REPLICA_TXID" \
  --arg shutdownTxid "$(jq -er '.txid' "$RUN_ROOT/restores/shutdown-result.json")" \
  '{
    formatVersion: 1,
    runId: $runId,
    remotePath: $remotePath,
    fencedTxid: $fencedTxid,
    remoteConfirmedTxid: $remoteTxid,
    exactFenceRestore: "PASSED",
    postCompactionExactRestore: "PASSED",
    requiredL0Retention: "720h",
    gracefulShutdownFinalSync: "PASSED",
    shutdownTxid: $shutdownTxid,
    orphanProcess: false
  }' >"$RUN_ROOT/report.json"
chmod 600 "$RUN_ROOT/report.json"

echo "Litestream R2 protocol verification passed"
echo "local evidence: $RUN_ROOT"
echo "remote evidence: s3://$DARA_LITESTREAM_R2_BUCKET/$REMOTE_PATH"
echo "the unique remote test prefix is retained for inspection"
