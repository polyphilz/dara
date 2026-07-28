#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
APP_ROOT="$ROOT/app"
ENV_FILE=${DARA_LITESTREAM_ENV_FILE:-"$APP_ROOT/.env.local"}
STAGED_BINARY="$APP_ROOT/src-tauri/resources/release/bin/litestream"

for command in cargo curl file jq otool shasum tar uuidgen; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

test "$(uname -s)" = Darwin || {
  echo "the real-R2 canary requires macOS" >&2
  exit 1
}

if test -z "${DARA_LITESTREAM_R2_ACCOUNT_ID:-}"; then
  test -r "$ENV_FILE" || {
    echo "R2 canary environment file not found: $ENV_FILE" >&2
    echo "copy app/.env.example to app/.env.local and add bucket-scoped credentials" >&2
    exit 1
  }
  set -a
  # shellcheck disable=SC1090
  . "$ENV_FILE"
  set +a
fi

require_environment() {
  variable=$1
  value=$2
  test -n "$value" || {
    echo "missing required canary variable: $variable" >&2
    exit 1
  }
}

require_environment DARA_LITESTREAM_R2_ACCOUNT_ID "${DARA_LITESTREAM_R2_ACCOUNT_ID:-}"
require_environment DARA_LITESTREAM_R2_JURISDICTION "${DARA_LITESTREAM_R2_JURISDICTION:-}"
require_environment DARA_LITESTREAM_R2_BUCKET "${DARA_LITESTREAM_R2_BUCKET:-}"
require_environment DARA_LITESTREAM_R2_PREFIX "${DARA_LITESTREAM_R2_PREFIX:-}"
require_environment DARA_LITESTREAM_R2_ACCESS_KEY_ID "${DARA_LITESTREAM_R2_ACCESS_KEY_ID:-}"
require_environment DARA_LITESTREAM_R2_SECRET_ACCESS_KEY "${DARA_LITESTREAM_R2_SECRET_ACCESS_KEY:-}"

"$ROOT/scripts/stage-litestream-sidecar.sh" >/dev/null

RUN_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
EVIDENCE_PARENT="$APP_ROOT/.data/r2-canary"
EVIDENCE_ROOT="$EVIDENCE_PARENT/$RUN_ID"
mkdir -p "$EVIDENCE_PARENT"

export DARA_RUN_R2_CANARY=1
export DARA_LITESTREAM_PATH="$STAGED_BINARY"
export DARA_R2_CANARY_DATA_DIR="$EVIDENCE_ROOT"

(
  cd "$APP_ROOT/src-tauri"
  cargo test \
    --locked \
    --lib \
    backup::restore::tests::live_r2_canary_restores_complete_checkpoint_and_cleans_unique_prefix \
    -- \
    --ignored \
    --exact \
    --nocapture \
    --test-threads=1
)

test -f "$EVIDENCE_ROOT/canary-report-v1.json" || {
  echo "R2 canary did not produce its bounded local report" >&2
  exit 1
}

echo "Dara real-R2 canary passed"
echo "local evidence: $EVIDENCE_ROOT"
echo "the unique remote canary prefix was removed and verified empty"
