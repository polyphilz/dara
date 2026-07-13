#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
MANIFEST="$ROOT/app/src-tauri/resources/embedding-indexes/jina-v1.json"
FIXTURES="$ROOT/app/src-tauri/resources/embedding-indexes/jina-v1-golden.json"
MODEL_FILE=${1:-"$HOME/Library/Application Support/dara/models/v5-nano-retrieval-Q8_0.gguf"}
LLAMA_EMBEDDING=${LLAMA_EMBEDDING:-llama-embedding}
LLAMA_SERVER=${LLAMA_SERVER:-llama-server}
LLAMA_DEVICE=${LLAMA_DEVICE:-none}
LLAMA_GPU_LAYERS=${LLAMA_GPU_LAYERS:-0}

for command in curl jq shasum "$LLAMA_EMBEDDING" "$LLAMA_SERVER"; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

test -f "$MODEL_FILE" || {
  echo "model not found: $MODEL_FILE" >&2
  echo "download and verify it using the commands in README.md" >&2
  exit 1
}

EXPECTED_FILE=$(jq -r '.config.modelFile' "$MANIFEST")
EXPECTED_SIZE=$(jq -r '.config.modelFileSize' "$MANIFEST")
EXPECTED_SHA=$(jq -r '.modelFileSha256' "$MANIFEST")
FIXTURE_SHA=$(jq -r '.modelFileSha256' "$FIXTURES")
ACTUAL_SIZE=$(wc -c <"$MODEL_FILE" | tr -d ' ')

test "$FIXTURE_SHA" = "$EXPECTED_SHA" || {
  echo "fixture model hash does not match the canonical manifest" >&2
  exit 1
}
test "$ACTUAL_SIZE" = "$EXPECTED_SIZE" || {
  echo "unexpected size for $EXPECTED_FILE: $ACTUAL_SIZE" >&2
  exit 1
}
printf '%s  %s\n' "$EXPECTED_SHA" "$MODEL_FILE" | shasum -a 256 -c -

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/dara-jina-v1.XXXXXX")
SERVER_PID=

cleanup() {
  if test -n "$SERVER_PID"; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

verify_output() {
  name=$1
  output=$2
  label=$3

  jq -e --arg name "$name" --slurpfile actual "$output" '
    def vector_norm: map(. * .) | add | sqrt;
    def dot($left; $right):
      reduce range(0; $left | length) as $index
        (0; . + ($left[$index] * $right[$index]));
    def max_absolute_difference($left; $right):
      [range(0; $left | length) as $index
        | (($left[$index] - $right[$index]) | fabs)] | max;

    .tolerance as $tolerance
    | (.cases[] | select(.name == $name) | .embedding) as $expected
    | $actual[0].data[0].embedding as $observed
    | ($observed | length) == 768
      and (($observed | vector_norm) - 1.0 | fabs) <= 0.00001
      and (dot($expected; $observed) >= $tolerance.minimumCosineSimilarity)
      and (max_absolute_difference($expected; $observed) <= $tolerance.maximumAbsoluteDifference)
  ' "$FIXTURES" >/dev/null
  echo "$name fixture passed through $label"
}

run_fixture() {
  name=$1
  prompt=$(jq -r --arg name "$name" '.cases[] | select(.name == $name) | .input' "$FIXTURES")
  output="$TMP_DIR/$name.json"

  "$LLAMA_EMBEDDING" \
    --model "$MODEL_FILE" \
    --pooling last \
    --embd-normalize 2 \
    --embd-output-format json \
    --device "$LLAMA_DEVICE" \
    --n-gpu-layers "$LLAMA_GPU_LAYERS" \
    --seed 0 \
    --prompt "$prompt" \
    >"$output"

  verify_output "$name" "$output" llama-embedding
}

run_fixture query
run_fixture document

PORT=${DARA_JINA_TEST_PORT:-$((40000 + ($$ % 20000)))}
SERVER_URL="http://127.0.0.1:$PORT"
SERVER_LOG="$TMP_DIR/llama-server.log"
"$LLAMA_SERVER" \
  --model "$MODEL_FILE" \
  --embedding \
  --pooling last \
  --embd-normalize 2 \
  --device "$LLAMA_DEVICE" \
  --n-gpu-layers "$LLAMA_GPU_LAYERS" \
  --parallel 1 \
  --host 127.0.0.1 \
  --port "$PORT" \
  >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

attempt=0
until curl -fsS --connect-timeout 1 "$SERVER_URL/health" >/dev/null 2>&1; do
  if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    cat "$SERVER_LOG" >&2
    echo "llama-server exited before becoming healthy" >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  if test "$attempt" -ge 100; then
    cat "$SERVER_LOG" >&2
    echo "llama-server did not become healthy" >&2
    exit 1
  fi
  sleep 0.1
done

SERVER_PROMPT=$(jq -r '.cases[] | select(.name == "query") | .input' "$FIXTURES")
jq -nc --arg input "$SERVER_PROMPT" '{input: $input}' \
  | curl -fsS \
      -H 'Content-Type: application/json' \
      --data-binary @- \
      "$SERVER_URL/v1/embeddings" \
      >"$TMP_DIR/server-query.json"
verify_output query "$TMP_DIR/server-query.json" llama-server

kill "$SERVER_PID" >/dev/null 2>&1 || true
wait "$SERVER_PID" >/dev/null 2>&1 || true
SERVER_PID=

echo "Jina v1 artifact and embedding fixtures passed"
