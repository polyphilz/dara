# dara

A personal spaced-repetition app for macOS. Built to replace Anki after 6–7 years of use — keeping what works (FSRS scheduling, atomic cards) and fixing what doesn't (slow, bloated, and enough friction in card creation that cards don't get made).

## Principles

- **Fast and resident.** Lives in the menu bar (Docker-style), launches instantly, costs ~nothing at idle.
- **Capture in five seconds.** A global hotkey shows an ephemeral Quick Add window over the current workspace and puts the caret in the editor. Dara remains a menu-bar-only Accessory app with no Dock icon or application menu. Save or cancel and keyboard control returns immediately to the app and window you were using.
- **Keyboard-first everywhere.** Reviews, editing, search, occlusion editing — time off the keyboard is the enemy.
- **Local-first.** One relational SQLite database plus one blob-only media database. No server, sync, or required account. Capture, review, and lexical search work immediately offline; semantic search runs locally after a background model download. Optional R2 backups and the AI mistake-explainer are the only networked features.
- **AI is advisory, never generative.** The AI explains *why you got a card wrong* (you tell it your reasoning, it finds the fault in your understanding). It never writes cards and never touches scheduling.
- **No feature bloat.** No decks (one interleaved pool). No tags (search is the organization). Three card types, nothing else.

## What v1 is

- FSRS scheduling (same modern algorithm Anki uses), desired-retention as the single user-facing knob
- Card types: markdown front/back, cloze deletion, image occlusion (N masks → N sibling cards)
- Ephemeral Quick Add window (global hotkey, restores the prior app/window on dismiss) + full app window for reviewing/editing/searching
- Hybrid search on Enter: lexical FTS5 + local text embeddings fused with Reciprocal Rank Fusion
- OCR on pasted images, so text inside screenshots is searchable
- Edit / suspend / unsuspend / delete / undo-last-grade
- AI mistake-explainer (BYOK or `codex exec`)

## What v1 is not

Mobile, sync, decks, tags, AI card generation, or a hosted service. UUIDs, tombstones, and append-only history preserve sync-compatible foundations, but no merge protocol or sync guarantee exists. Anki import is specced but optional — a fresh start is the likely path.

## Stack

Tauri v2. TypeScript UI (React + ProseMirror, with CodeMirror 6 nested in code blocks) + `ts-fsrs`; Rust layer for data (rusqlite + statically linked `sqlite-vec` + FTS5), the FSRS optimizer (`fsrs-rs`), a llama.cpp sidecar running Jina v5 nano retrieval embeddings, and macOS activation/focus-restoration glue around standard Tauri windows. State is two SQLite files (relational data + immutable media blobs) with an append-only review log as the scheduling source of truth.

## Reproducing the v1 text-embedding pin

The v1 semantic index is defined by the model file and the settings that give its vectors meaning. Dara pins those inputs so it never mixes incompatible embeddings in one vec0 table. The `llama.cpp` sidecar is tested and recorded per Dara release, but it is not part of the index identity: a compatible sidecar upgrade does not require rebuilding every vector.

The v1 index inputs are:

| Input | Pinned value |
| --- | --- |
| GGUF repository | `jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF` |
| GGUF revision | `59cfaceeeb7d738c404659435af4c0da74d06c96` |
| GGUF file | `v5-nano-retrieval-Q8_0.gguf` |
| GGUF size | `232883776` bytes |
| GGUF SHA-256 | `86b6e6279e9b9e71389f02a082764a2ac2b15a50e37482c26f98d69092f12442` |
| Vector configuration | 768 dimensions, last-token pooling, L2 normalization |
| Retrieval prefixes | queries: `Query: `; documents: `Document: ` |

The official repository manifest can be checked with `curl` and `jq`:

```sh
curl -sS \
  'https://huggingface.co/api/models/jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF?blobs=true' \
  | jq '{
      revision: .sha,
      q8: (.siblings[]
        | select(.rfilename == "v5-nano-retrieval-Q8_0.gguf")
        | {file: .rfilename, size, sha256: .lfs.sha256})
    }'
```

Download by immutable revision and verify the bytes before loading the model:

```sh
MODEL_DIR="$HOME/Library/Application Support/dara/models"
MODEL_FILE="$MODEL_DIR/v5-nano-retrieval-Q8_0.gguf"
MODEL_REVISION='59cfaceeeb7d738c404659435af4c0da74d06c96'
MODEL_SHA256='86b6e6279e9b9e71389f02a082764a2ac2b15a50e37482c26f98d69092f12442'

mkdir -p "$MODEL_DIR"
curl -fL --retry 3 \
  "https://huggingface.co/jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF/resolve/$MODEL_REVISION/v5-nano-retrieval-Q8_0.gguf" \
  -o "$MODEL_FILE"
printf '%s  %s\n' "$MODEL_SHA256" "$MODEL_FILE" | shasum -a 256 -c -
```

### Local development

Repository-local model files belong in the ignored `models/` directory. The application normally
downloads and verifies the pinned model beneath its Dara data directory; development can reuse a
checked artifact and a local llama.cpp build explicitly:

```sh
cd app
DARA_EMBEDDING_MODEL_PATH="$PWD/../models/v5-nano-retrieval-Q8_0.gguf" \
DARA_LLAMA_SERVER_PATH=/opt/homebrew/bin/llama-server \
  pnpm tauri dev
```

The development command keeps its databases under `app/.data/local/`. The model override changes
only where model bytes are read from. Dara runs the manifest checksum and golden compatibility
checks before first use and whenever the model, sidecar, inference settings, or verification
contract changes. A successful check writes an atomic, machine-local derived receipt beneath the
Dara data directory. An exact receipt match keeps the model unloaded until a hybrid query or stale
document embedding actually needs inference; a runtime failure invalidates the receipt.

### Reproducing the sidecar

Dara's arm64 production build bundles `llama-server` and downloads the larger GGUF separately.
The canonical source revision, target, CMake flags, and license notice live in
`app/src-tauri/resources/sidecars/llama-server-v1.json`. The v1 pin is upstream commit
`fdb1db877c526ec90f668eca1b858da5dba85560` (build 9860). Homebrew's formula moves over
time and is never consulted by a release build.

The release staging script checks out that exact revision, builds a static arm64 executable with
embedded Metal shaders, runs the Jina compatibility gate through CPU and Metal, verifies that the
binary has no non-system dynamic dependencies, and writes an ignored staging directory containing
the executable, MIT license, and a machine-readable release manifest:

```sh
./scripts/build-llama-sidecar.sh \
  ./models/v5-nano-retrieval-Q8_0.gguf
```

The staged files are explicit Tauri resources. Build the ad-hoc `.app` from `app/` with:

```sh
pnpm release:build:app
```

See [`docs/RELEASE.md`](docs/RELEASE.md) for the complete versioning, build,
installation, smoke-check, and tagging procedure.

This personal-v1 command targets arm64 macOS 14 or newer. It intentionally rebuilds and rechecks
the sidecar before packaging. The GGUF remains outside the `.app` and is downloaded and verified
under Dara's data directory on first semantic use.

### Compatibility check

Run a fixed input with the same pooling, normalization, and prefix Dara uses. The basic check confirms that the model loads and returns one normalized 768-dimensional vector:

```sh
LLAMA_EMBEDDING="$LLAMA_CPP_DIR/build/bin/llama-embedding"

"$LLAMA_EMBEDDING" \
  --model "$MODEL_FILE" \
  --pooling last \
  --embd-normalize 2 \
  --embd-output-format json \
  --device none \
  --n-gpu-layers 0 \
  --seed 0 \
  --prompt 'Query: Why does spaced repetition work?' \
  > /tmp/dara-jina-query.json

jq -e '.data[0].embedding | length == 768' /tmp/dara-jina-query.json
jq '[.data[0].embedding[] | . * .] | add | sqrt' /tmp/dara-jina-query.json
```

Dara keeps one or two small known-output fixtures—one query and one document—to catch mistakes such as dropping a prefix or changing the pooling mode. A sidecar upgrade must produce compatible results for those fixtures, within a small tolerance, and pass through the same `llama-server --embedding --pooling last --embd-normalize 2` endpoint used by the application. The fixtures test Dara's integration with the official GGUF; Dara does not independently validate Jina's quantization against the original high-precision model.

Run the complete artifact, fixture, and sidecar-endpoint check with:

```sh
LLAMA_EMBEDDING="$LLAMA_CPP_DIR/build/bin/llama-embedding" \
LLAMA_SERVER="$LLAMA_CPP_DIR/build/bin/llama-server" \
  ./scripts/verify-jina-v1.sh "$MODEL_FILE"
```

The script defaults to the CPU backend for reproducibility. Before packaging a macOS release, run the same gate through Metal as well:

```sh
LLAMA_DEVICE=auto LLAMA_GPU_LAYERS=all \
LLAMA_EMBEDDING="$LLAMA_CPP_DIR/build/bin/llama-embedding" \
LLAMA_SERVER="$LLAMA_CPP_DIR/build/bin/llama-server" \
  ./scripts/verify-jina-v1.sh "$MODEL_FILE"
```

After this check passes, the V1 migration seeds the immutable `TextEmbeddingIndex` definition but leaves `AppSettings.active_text_embedding_index_id` null. Dara activates it only after the artifact is verified and every active `SearchDocument` has a current vector.

A new upstream GGUF or `llama.cpp` commit does not itself create a new index. Dara creates a new vec0 table only when it intentionally adopts model bytes or semantic settings that produce incompatible vectors. It then builds the replacement in the background and switches the active pointer when the new index is complete.

## Status

Early development. The macOS windowing and SQLite foundations are implemented, together with all three card types, images/OCR, saved-card editing, suspension, tombstone deletion, and Enter-only local hybrid search. FSRS configurability and resilience work, the AI explainer, and distribution remain under construction.
