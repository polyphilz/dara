# Packaged release acceptance

Status: **not yet run for the current release artifact**.

This procedure owns two release-level claims that unit, browser, and e2e builds cannot make:

1. a packaged Dara starts from an empty repository-local data directory, resumes and verifies its
   managed model, indexes without blocking the core loop, reuses its verification receipt, and
   cleans up the bundled sidecar; and
2. the same packaged app upgrades a rich previous-head database pair only after preserving a
   valid, restorable pre-migration snapshot.

The driver inspects durable files and database state after Dara has quit. UI observations remain
explicit below; a file check does not claim that a user interaction worked.

## Prerequisites

- Apple Silicon on macOS 14 or newer with network access for the clean model download.
- No other Dara process running. Quit development, e2e, and packaged copies first.
- A freshly built and verified app:

  ```sh
  cd app
  pnpm release:build:app
  ```

- Run every command from `app/`. Data names are resolved as direct children of `app/.data/`.
  Existing directories are refused; the harness never deletes or resets test data.
- The `launch` command directly starts the packaged executable with `DARA_DATA_DIR` set to the
  selected test directory. It removes all model, sidecar, device, and GPU-layer development
  overrides from the child environment.

Use unique names in place of the examples below.

## A. Clean first run

### A1. Interrupt and resume the managed download

Prepare and launch a genuinely empty directory:

```sh
pnpm release:acceptance prepare-clean release-clean-20260727
pnpm release:acceptance launch release-clean-20260727
```

Before the download completes:

- invoke Home and confirm the main window was not shown merely because the resident app launched;
- create at least two cards, including “mitochondria produce cellular energy” or equivalent;
- confirm Browse-all contains them;
- run an exact lexical query and confirm it works while semantic search is still downloading;
- reveal and grade one card; and
- confirm Settings reports increasing downloaded bytes.

Quit with Cmd+Q while the download is incomplete, then run:

```sh
pnpm release:acceptance check-interrupted-download release-clean-20260727
```

The check requires a non-empty, incomplete `.gguf.part`, no completed model or verification
receipt, no sidecar log/PID, and durable card/search data. If the download finishes too quickly,
use a new clean directory and interrupt earlier.

Relaunch the same directory:

```sh
pnpm release:acceptance launch release-clean-20260727
```

Confirm the displayed byte count resumes above zero rather than restarting. While it finishes,
confirm Quick Add, Browse, review, and lexical search remain responsive. After Settings reports
semantic search as Ready:

- confirm all authored search documents become indexed together;
- search for a semantic paraphrase with no exact lexical overlap, such as “powerhouse of the
  cell,” and confirm the expected mitochondria card is returned; and
- leave both windows hidden briefly, then show them again and confirm the app remains responsive.

Quit with Cmd+Q and run:

```sh
pnpm release:acceptance check-clean release-clean-20260727
```

This verifies the pinned model size and SHA-256, the bundled manifest/golden-fixture hashes named
by the verification receipt, the bundled sidecar path, current migration heads, complete
embedding counts and atomic index activation, a finalized launch snapshot, released app-data
lock, and absence of a sidecar PID/process after normal exit. A receipt exists only after checksum
and golden-fixture verification succeeds.

### A2. Verification-receipt reuse

Record the completed state:

```sh
pnpm release:acceptance checkpoint-restart release-clean-20260727
pnpm release:acceptance launch release-clean-20260727
```

On this launch, wait for Settings to report Ready but do not perform a search, because a user
query intentionally starts the lazy sidecar. Quit with Cmd+Q and run:

```sh
pnpm release:acceptance check-restart release-clean-20260727
```

The check requires byte-for-byte and timestamp-stable receipt/model files and an unchanged
sidecar log. Together those prove the clean restart reused the receipt instead of rehashing,
rerunning golden inference, or starting `llama-server`.

## B. Upgrade acceptance

### B1. Build and open the previous-head fixture

The fixture builder is checked Rust test code. It creates main V6/media V1 databases containing:

- active BASIC, CLOZE, and OCCLUSION content plus a tombstoned card;
- multiple variants, one suspended card, a review and its revocation;
- a valid image/media blob pair and occlusion geometry;
- lexical search projections; and
- dark appearance, 130% zoom, and recognizable legacy shortcuts.

Build it beneath `app/.data/`:

```sh
pnpm release:acceptance prepare-upgrade release-upgrade-20260727
pnpm release:acceptance launch release-upgrade-20260727
```

In the packaged app:

- confirm Browse shows the Basic, Cloze, and image-occlusion fixtures;
- confirm the cloze variants, suspended state, image, and occlusion mask still render;
- confirm deleted content remains absent;
- confirm dark appearance and 130% zoom survive;
- confirm the legacy Review shortcut migrated to the Home command without losing its custom
  accelerator; and
- confirm lexical search finds “release acceptance.”

Quit with Cmd+Q and run:

```sh
pnpm release:acceptance check-upgrade release-upgrade-20260727
```

The check requires both live migration heads to match the current source, all expected authored
and derived rows to survive, a valid V6/V1 pre-migration snapshot, and a valid current-head launch
snapshot. It invokes the packaged binary’s offline `recovery verify` command against the
pre-migration manifest.

### B2. Reopen and restore proof

Launch the upgraded directory again, repeat a short Browse/Settings check, quit, and rerun
`check-upgrade`. Then prove the preserved snapshot can actually be installed into a separate,
empty repository-local directory:

```sh
pnpm release:acceptance launch release-upgrade-20260727
# inspect, then Cmd+Q
pnpm release:acceptance check-upgrade release-upgrade-20260727
pnpm release:acceptance prove-upgrade-restore \
  release-upgrade-20260727 \
  release-upgrade-restored-20260727
```

The restore proof never replaces the upgraded run. It asks the packaged recovery command to
restore the V6/V1 manifest into the new target and then checks the old heads, authored cards,
variants, media digest, review history, search projections, settings, and legacy shortcut.

## Release record

Record the final run here or in a dated successor before calling the artifact ready:

| Field | Result |
| --- | --- |
| Dara version / commit | pending |
| `.app` SHA-256 | pending |
| macOS / hardware | pending |
| Tester / date | pending |
| Clean download interruption and resume | pending |
| Immediate capture/review/Browse/lexical workflow | pending |
| Golden verification, indexing, and hybrid search | pending |
| Receipt reuse and clean sidecar shutdown | pending |
| V6/V1 snapshot-before-migration upgrade | pending |
| Reopen and offline restore proof | pending |

Do not mark a row passed solely because a lower-level test is green. Preserve the task-specific
directories until the release record and any failure evidence have been reviewed.
