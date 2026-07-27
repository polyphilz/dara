# Releasing Dara

This is the authoritative runbook for building and installing a personal Dara
release. Follow it from a clean checkout of `main`; do not infer Dara's release
process from another Tauri repository.

## Current release policy

- Dara releases are built locally on the release Mac.
- The supported personal-v1 artifact is an arm64 `.app` for macOS 14 or newer.
- The artifact is ad-hoc signed. It is suitable for local use, but it is not a
  notarized public distribution.
- Dara does not currently have a tag-triggered GitHub release workflow, an
  updater, or a DMG release target.
- A release tag records the exact source commit. Pushing a tag does not build,
  upload, or publish Dara.
- The GGUF model is not bundled. The installed app downloads and verifies it
  beneath Dara's production data directory when semantic search needs it.
- The pinned `llama-server` binary, embedding manifests, golden fixtures, and
  llama.cpp license are bundled and verified by the release build.

Do not substitute `pnpm tauri build` for the release command. The ordinary
Tauri configuration does not stage and verify the production sidecar.

## Production and development data

The installed app uses:

```text
~/Library/Application Support/dara
```

`pnpm tauri dev` sets `DARA_DATA_DIR` and uses:

```text
app/.data/local
```

Replacing `/Applications/Dara.app` does not replace or reset the production
data directory. Never copy, delete, or overwrite production data as part of a
normal app release.

When testing a release candidate before installation, use the acceptance
commands and a task-specific directory beneath `app/.data/`. Do not launch a
candidate directly against the production directory.

## Version sources

Keep the same semantic version in all three files:

- `app/package.json`
- `app/src-tauri/Cargo.toml`
- `app/src-tauri/tauri.conf.json`

The Tauri version becomes the macOS bundle version. The Rust package version is
also part of Dara's diagnostics, snapshot manifests, and recovery records, so
it is not merely internal metadata.

After changing `Cargo.toml`, let Cargo update Dara's package entry in
`app/src-tauri/Cargo.lock` without broadly upgrading dependencies:

```sh
cd app
cargo check --manifest-path src-tauri/Cargo.toml
```

Confirm the three configured versions before merging:

```sh
cd app
jq -r .version package.json
jq -r .version src-tauri/tauri.conf.json
cargo metadata --no-deps --format-version 1 \
  --manifest-path src-tauri/Cargo.toml \
  | jq -r '.packages[] | select(.name == "dara") | .version'
```

For the first `v0.1.0` release, the files already have the correct version. Do
not create an unnecessary version-only change.

## 1. Prepare the release commit

1. Make the intended application changes on a branch.
2. Choose the next version and update the three version sources.
3. Run the normal checks from `app/`:

   ```sh
   pnpm check
   cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
   cargo clippy --locked --manifest-path src-tauri/Cargo.toml \
     --all-targets --all-features -- -D warnings
   pnpm test:native-bundle-safety
   ```

4. Open and merge a PR after the required GitHub checks pass.

Do not build a release from a dirty worktree, an unreviewed feature branch, or
a commit that differs from the commit intended for the version tag.

## 2. Prepare the release checkout

From the repository root:

```sh
git switch main
git pull --ff-only
git status --short --branch
```

The status must show `main` aligned with `origin/main` and no tracked or
untracked changes. Record the source commit:

```sh
git rev-parse HEAD
```

Quit every running Dara instance, including `pnpm tauri dev`, before installing
or smoke-testing the production app.

## 3. Ensure the verification model exists

The release build expects this ignored, repository-local file:

```text
models/v5-nano-retrieval-Q8_0.gguf
```

Its revision, byte length, and SHA-256 are pinned in the root `README.md` and
`app/src-tauri/resources/embedding-indexes/jina-v1.json`. If the file is
missing, download the immutable revision using the README instructions, place
it at the path above, and verify its checksum.

This local model is used to verify the sidecar through both CPU and Metal. It
is not copied into the `.app`.

## 4. Build and verify the packaged app

Run the release command from `app/`:

```sh
cd app
pnpm release:build:app
```

The command:

1. fetches the pinned llama.cpp revision into a temporary directory;
2. builds arm64 `llama-server` with embedded Metal shaders;
3. checks the pinned model and golden fixtures through CPU and Metal;
4. rejects non-system dynamic sidecar dependencies;
5. stages the sidecar, manifest, fixtures, and license;
6. builds Dara with `src-tauri/tauri.release.conf.json`;
7. ad-hoc signs the app and nested sidecar; and
8. verifies architecture, minimum macOS version, hashes, executable bits,
   bundled resources, and production/test isolation.

The verified artifact is:

```text
app/src-tauri/target/release/bundle/macos/Dara.app
```

A successful command ends with a `Packaged app passed` message. Any earlier
failure means there is no releasable artifact.

## 5. Archive the exact candidate

Keep the candidate until the installed smoke check passes. From `app/`, set the
version explicitly and create a zip outside the repository:

```sh
DARA_RELEASE_VERSION=0.1.0
DARA_RELEASE_APP=src-tauri/target/release/bundle/macos/Dara.app
DARA_RELEASE_ARCHIVE="$HOME/Downloads/Dara-$DARA_RELEASE_VERSION-macos-arm64.zip"

ditto -c -k --sequesterRsrc --keepParent \
  "$DARA_RELEASE_APP" \
  "$DARA_RELEASE_ARCHIVE"
shasum -a 256 "$DARA_RELEASE_ARCHIVE"
```

Record the source commit and archive SHA-256 together. The archive is the
rollback copy of the application bundle; it does not contain user data.

## 6. Install over the current production app

1. Quit Dara with `Cmd+Q`. Confirm no Dara or `llama-server` process remains.
2. Reveal
   `app/src-tauri/target/release/bundle/macos/Dara.app` in Finder.
3. Drag it into `/Applications`.
4. If Finder asks, choose **Replace**.
5. Launch `/Applications/Dara.app` from Finder, Spotlight, or Raycast.

Do not launch the installed app with `DARA_DATA_DIR`,
`DARA_LLAMA_SERVER_PATH`, or `DARA_EMBEDDING_MODEL_PATH` overrides. Those are
development and acceptance mechanisms, not production configuration.

On an upgrade, Dara reuses the existing production data directory. If database
migrations are pending, startup creates and validates a paired pre-migration
snapshot before applying them.

## 7. Run the installed smoke check

Use the app from `/Applications`, not the copy under `target/`.

- Confirm the menu-bar app starts and both global shortcuts work.
- Create or inspect a card through Quick Add and Browse.
- Reveal, grade, and undo one review.
- Exercise copy, paste, and undo in an editor.
- Run a lexical search.
- Run a semantic search and confirm model download/verification succeeds if
  this is the first production run.
- Change one harmless persisted setting, quit with `Cmd+Q`, reopen, and confirm
  both data and the setting remain.
- After quitting, confirm no `llama-server` process remains.

If the smoke check fails, do not tag the commit. Reinstall the previously
archived app while leaving the production data directory untouched. If the
failure involved a database migration, investigate the pre-migration snapshot
before reopening repeatedly.

## 8. Tag the successful release

Only tag after the installed artifact passes the smoke check:

```sh
git switch main
git tag -a v0.1.0 -m "dara v0.1.0"
git push origin v0.1.0
```

The tag version must match the three version sources. Never move a tag that has
been treated as a released version; fix the issue and cut a new patch version.

Again, Dara currently has no workflow listening for this tag. The push records
the source release on GitHub but does not publish the archived `.app`.

## Recovery boundary

Normal releases never manipulate files inside the production data directory.
If an upgrade truly requires snapshot recovery:

- Dara must be completely closed.
- Recovery must be an explicit operator action, not part of installation.
- List and verify a snapshot before restoring it.
- Preserve the failed data and logs until the cause is understood.

The packaged binary exposes:

```text
dara recovery list <data-directory>
dara recovery verify <manifest>
dara recovery restore <manifest> <data-directory>
```

These commands can overwrite production database state. An LLM or automation
must not run them against `~/Library/Application Support/dara` without the
user's explicit approval for that exact recovery.

## Public distribution is a separate milestone

Uploading the current artifact for other people would distribute an arm64,
ad-hoc-signed app. Downloaded copies would be quarantined by macOS and would not
provide the normal signed-and-notarized installation experience.

Before treating Dara as a public binary release, deliberately add:

- Developer ID signing for Dara and the nested `llama-server`;
- hardened-runtime entitlement validation;
- notarization and stapling;
- a DMG or another documented installation artifact;
- a clean-machine Gatekeeper test;
- an explicit architecture/support policy; and
- a release workflow that preserves Dara's sidecar and model-verification
  gates.

Do not copy another app's generic `tauri-action` workflow without reproducing
the guarantees in `pnpm release:build:app`.
