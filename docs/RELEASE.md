# Releasing Dara

This is the authoritative runbook for building and installing Dara. Follow it
from a clean checkout of `main`; do not infer Dara's release process from
another Tauri repository.

For user-facing setup, privacy, restore, and decommissioning guidance, see
[`OFFSITE_BACKUP.md`](OFFSITE_BACKUP.md).

## Current release policy

- Dara releases are built locally on the release Mac.
- The supported personal artifact is an arm64 `.app` for macOS 14 or newer.
  It remains ad-hoc signed for fast local release and acceptance work.
- The public artifact is an arm64 Developer ID signed, hardened, notarized, and
  stapled `.dmg` for macOS 14 or newer.
- Apple signing, notarization, updater signing, and release artifact creation
  happen only on the release Mac. GitHub Actions never receives those private
  credentials.
- GitHub Releases is the public distribution source. Each release contains the
  DMG for new installs plus the signed Tauri updater archive, signature,
  `latest.json`, and `SHA256SUMS` for existing installs.
- A release tag records the exact source commit. Pushing a tag does not build
  or publish Dara; a local release command uploads already-verified artifacts
  to a draft GitHub Release.
- The GGUF model is not bundled. The installed app downloads and verifies it
  beneath Dara's production data directory when semantic search needs it.
- The pinned `llama-server` and Litestream binaries, embedding manifests,
  golden fixtures, licenses, and Litestream notice are bundled and verified by
  the release build.

Do not substitute `pnpm tauri build` for the release command. The ordinary
Tauri configuration does not stage and verify the production sidecars.

## What existing users see

A packaged production build checks the latest published GitHub Release five
seconds after launch and every six hours while it remains open. A newer version
appears as a small notification in the bottom-left corner with **Install and
restart** and **Not now**. Deferring hides that version for 24 hours. Automatic
network failures remain silent.

Users can also choose **Check for Updates…** from either the Dara application
menu or the brain menu-bar icon. Manual checks report whether Dara is current
or explain the error. Before an installed update relaunches, Dara uses its
normal graceful exit path to stop Litestream, media reconciliation, semantic
search, and managed sidecars.

The updater trusts two independent protections:

- Apple Developer ID signing and notarization let macOS trust the application.
- Dara's Tauri updater signature proves that an archive was signed with the
  private updater key paired with the public key embedded in Dara.

Publishing the GitHub Release makes its `latest.json` visible. That publication
is therefore the moment existing installations can discover the update.

## Production and development data

The installed app uses:

```text
~/Library/Application Support/dara
```

`pnpm tauri dev` sets `DARA_DATA_DIR` and uses:

```text
app/.data/local
```

That command runs **Dara Local** with bundle identifier
`com.silo77.dara.local`. Development and production use separate macOS
application, single-instance, autostart, logging, and R2 Keychain identities.
Non-production builds refuse to start without an explicit `DARA_DATA_DIR`.
The production bundle identifier is `com.silo77.dara`. Builds using the new
SILO77 identifiers migrate any saved R2 credentials from Dara's former
`com.rohan.dara` Keychain service after verifying the replacement entry.

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
   pnpm release:verify-contracts
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

1. checks the pinned Litestream, packaging, retention, license, and canary
   contracts;
2. fetches the pinned llama.cpp revision into a temporary directory;
3. builds arm64 `llama-server` with embedded Metal shaders;
4. checks the pinned model and golden fixtures through CPU and Metal;
5. downloads the exact Litestream v0.5.15 arm64 release asset and verifies its
   official archive digest plus the pinned extracted-binary digest;
6. rejects non-system dynamic sidecar dependencies;
7. stages both sidecars, manifests, fixtures, licenses, and notice;
8. builds Dara with `src-tauri/tauri.release.conf.json`;
9. ad-hoc signs the app and nested sidecars; and
10. verifies architecture, minimum macOS version, hashes, executable bits,
   bundled resources, and production/test isolation.

The verified artifact is:

```text
app/src-tauri/target/release/bundle/macos/Dara.app
```

A successful command ends with a `Packaged app passed` message. Any earlier
failure means there is no releasable artifact.

### Build the public Developer ID distribution

This one-time Mac setup must already be complete:

- the Keychain contains the valid `Developer ID Application: SILO77 LLC
  (PMZH6ULML8)` certificate and its private key; and
- the App Store Connect API `.p8` key is stored outside the repository with
  owner-only permissions.
- the password-protected Tauri updater private key is stored outside the
  repository, with a backup of both the key and password; and
- the matching Tauri updater public key is embedded in
  `src-tauri/tauri.conf.json`.

Generate that updater key pair once, outside the repository:

```sh
mkdir -p "$HOME/Library/Application Support/SILO77/Dara Release"
chmod 700 "$HOME/Library/Application Support/SILO77/Dara Release"
cd app
pnpm tauri signer generate --write-keys \
  "$HOME/Library/Application Support/SILO77/Dara Release/dara-updater.key"
```

Choose a unique password at the prompt and save it in the password manager.
Back up `dara-updater.key` and its password. Losing that private key prevents
future Dara versions from updating installations that trust its paired public
key. The generated `.pub` file is intentionally public: copy its complete
contents into `plugins.updater.pubkey` in
`src-tauri/tauri.conf.json`. The public configuration is shared so the native
plugin can initialize in every build, but only official release commands set
the frontend marker that permits network update checks.

Copy the safe template and fill in the issuer ID, key ID, and absolute path to
that external key file:

```sh
cd app
cp .env.notarization.example .env.notarization
chmod 600 .env.notarization
```

`.env.notarization` and the `.p8` key are local secrets and must never be
committed. The example contains names and placeholders only.

Configure the separate Tauri updater signing key using the safe template:

```sh
cd app
cp .env.updater.example .env.updater
chmod 600 .env.updater
```

Set `TAURI_SIGNING_PRIVATE_KEY_PATH` to the absolute path of the external
updater private key and set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to its
password. `.env.updater` is ignored. It is loaded only by Dara's local artifact
script, and the password is passed to the signer through the child process
environment rather than a command-line argument.

The Apple `.p8`, Apple signing `.p12`, Tauri updater private key, their
passwords, `.env.notarization`, and `.env.updater` are secrets. The Apple
certificate, team/key/issuer identifiers, Tauri updater public key, release
signatures, checksums, and `latest.json` are safe to publish.

Build the public artifact from `app/`:

```sh
pnpm release:build:distribution
```

The command performs the normal pin, license, CPU, Metal, and resource checks
before any signing. It validates the updater private-key path, password, and
match with Dara's embedded public key before starting the expensive build or
submitting anything to Apple. It then:

1. signs `llama-server` and Litestream with fixed Dara sidecar identifiers,
   the SILO77 Developer ID identity, secure timestamps, and hardened runtime;
2. builds and signs Dara with hardened runtime;
3. submits the application to Apple, records its submission ID, waits for
   acceptance, and staples its ticket;
4. creates and signs the final drag-to-Applications DMG, submits it to Apple,
   records its submission ID, and staples its ticket;
5. mounts the exact DMG and rechecks its resources and Developer ID chain; and
6. asks Gatekeeper to assess both the disk image and the installed app;
7. archives that final notarized application for the Tauri updater;
8. signs the updater archive with the separate local updater key; and
9. produces the exact GitHub Release assets and checksums.

Apple signatures change executable bytes. Dara therefore verifies the exact
unsigned upstream sidecar hashes before signing, and the installed runtime
accepts Litestream only when it either still matches that exact hash or has the
fixed Dara Litestream identifier under the pinned SILO77 certificate and team.
A third-party or tampered signature does not satisfy that requirement.

The public artifact is:

```text
app/src-tauri/target/release/bundle/dmg/Dara_0.1.0_aarch64.dmg
```

A successful command ends with `Notarized distribution passed`. Do not upload
the DMG if any signing, notarization, stapling, mounted-content, or Gatekeeper
check fails.

The complete release-asset directory is:

```text
app/src-tauri/target/release/bundle/release/
├── Dara_0.1.0_aarch64.dmg
├── Dara_0.1.0_aarch64.app.tar.gz
├── Dara_0.1.0_aarch64.app.tar.gz.sig
├── SHA256SUMS
└── latest.json
```

The updater archive is deliberately created after Apple accepts and Dara
staples the application. Never substitute an earlier Tauri-generated archive:
Apple signing and stapling modify the bundle, so an archive made before those
steps is not the final application users should install.

Artifact creation verifies the updater archive and signature with the exact
public key embedded in Dara. It also records the source commit and whether the
worktree was dirty in `latest.json`. Draft publication rejects artifacts from
a different commit or a dirty build, preventing an old same-version bundle
from being attached to a newer source tag.

Submission state is kept beneath
`app/src-tauri/target/release/bundle/notarization/`. Status checks retry
transient Apple or network failures. If the command still stops after an
upload, preserve the app, DMG, archive, and state files, then resume without
rebuilding or resubmitting them:

```sh
pnpm release:resume:distribution
```

The resume command verifies that each saved upload still has the exact SHA-256
recorded at submission time before querying Apple or stapling anything. The
application state also records the signed sidecar SHA-256 values used by the
final verifier, so a resume does not require the ignored signing-staging
directory. When resuming older state that predates those values, Dara derives
them from the preserved submitted application archive and upgrades the state
before continuing.

To inspect an independently archived app and DMG, run:

```sh
pnpm release:verify-distribution -- <app> <dmg>
```

That standalone form compares the supplied app with the app mounted from the
DMG, authenticates and Gatekeeper-assesses both copies, and does not read
submission state left by a different build in the current checkout.

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

1. Quit Dara with `Cmd+Q`. Confirm no Dara, `llama-server`, or Litestream
   process remains.
2. Reveal
   `app/src-tauri/target/release/bundle/macos/Dara.app` in Finder.
3. Drag it into `/Applications`.
4. If Finder asks, choose **Replace**.
5. Launch `/Applications/Dara.app` from Finder, Spotlight, or Raycast.

Do not launch the installed app with `DARA_DATA_DIR`,
`DARA_LLAMA_SERVER_PATH`, `DARA_EMBEDDING_MODEL_PATH`, or
`DARA_LITESTREAM_PATH` overrides. Those are development and acceptance
mechanisms, not production configuration.

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
- After quitting, confirm no `llama-server` or Litestream process remains.

If the smoke check fails, do not tag the commit. Reinstall the previously
archived app while leaving the production data directory untouched. If the
failure involved a database migration, investigate the pre-migration snapshot
before reopening repeatedly.

## 8. Run packaged recovery acceptance

Follow [`app/tests/native/release-acceptance.md`](../app/tests/native/release-acceptance.md)
against the exact candidate. In addition to clean-install and migration checks,
the release record requires one dedicated R2 backup set to prove:

- the packaged app publishes a complete database-and-media checkpoint;
- a packaged restore drill can read that checkpoint;
- the packaged recovery command restores it into a new directory; and
- the restored installation can explicitly take over ownership and publish a
  checkpoint in a new ownership era.

The acceptance driver confines data to direct children of `app/.data/`.
Supply the five required `DARA_LITESTREAM_R2_*` variables only to the terminal running
the recovery proof; normal packaged launches deliberately remove them and must
read credentials from the macOS Keychain.

Do not point acceptance at a production bucket. The packaged app and recovery
command both use Dara's fixed `dara/primary` location. Preserve the local
evidence and manually delete that location from the disposable acceptance
bucket only after the release record has been reviewed.

## 9. Tag the successful release

Only tag after the installed artifact passes the smoke check:

```sh
git switch main
git tag -a v0.1.0 -m "dara v0.1.0"
git push origin v0.1.0
```

The tag version must match the three version sources. Never move a tag that has
been treated as a released version; fix the issue and cut a new patch version.

No GitHub workflow listens for this tag. The push records the source release
but does not upload or publish binaries.

## 10. Create and inspect the draft GitHub Release

Write user-facing release notes outside the repository, then upload the exact
local artifacts to a draft release:

```sh
cd app
pnpm release:publish:draft -- "$HOME/Downloads/dara-v0.1.0-release-notes.md"
```

The command refuses to continue unless:

- all three version sources agree;
- the worktree is clean and `main` exactly matches `origin/main`;
- an annotated `v0.1.0` tag points at that commit locally and on GitHub;
- every expected artifact exists;
- `latest.json`, the updater signature, and every SHA-256 agree.

It creates a **draft**, never a public release. In GitHub's draft UI, confirm
the tag, source commit, notes, and five assets. Download the draft DMG, compare
it with `SHA256SUMS`, install it on a second Mac if available, and repeat the
installed smoke check. Draft releases are invisible to the updater.

## 11. Publish the release

When the downloaded draft artifact passes acceptance, use GitHub's **Publish
release** action. Confirm it is marked as the latest release. The public release
page is the human-facing archive of every Dara DMG; no separate Markdown list
of downloads is maintained.

After publication:

```sh
curl --fail --location \
  https://github.com/polyphilz/dara/releases/latest/download/latest.json
gh release view v0.1.0 --json isDraft,tagName,assets,url
```

The first public release has no older public build to upgrade, so its updater
can only be checked manually for the “up to date” result. Starting with the
next patch release, keep the previous version installed on a test Mac, publish
the new version, and verify the complete notification, download, signature
verification, graceful restart, and data-preservation path.

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
dara recovery remote-inspect <latest|checkpoint-id>
dara recovery remote-restore <latest|checkpoint-id> <data-directory>
```

The local `restore` and remote `remote-restore` commands can overwrite database
state at their target. An LLM or automation must not run them against
`~/Library/Application Support/dara` without the user's explicit approval for
that exact recovery.

## Public distribution boundary

Only artifacts produced by `pnpm release:build:distribution` are supported for
public distribution or automatic updates. The faster
`pnpm release:build:app` output remains ad-hoc signed and is for local
installation and acceptance only; its updater frontend is disabled so it
cannot replace itself with a published production build.

The distribution command deliberately composes with, rather than replaces,
the existing sidecar and model-verification gates. GitHub Actions may continue
to test public source, but it does not receive the Apple or Tauri private keys
and cannot produce a public release. Keep that boundary unless the project's
release threat model is explicitly changed.
