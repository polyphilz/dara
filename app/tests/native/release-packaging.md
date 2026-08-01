# Arm64 release packaging record

Recorded on 2026-07-26 for Dara 0.1.0 on arm64 macOS.

## Build

- Canonical sidecar pin:
  `src-tauri/resources/sidecars/llama-server-v1.json`
- llama.cpp revision: `fdb1db877c526ec90f668eca1b858da5dba85560` (build 9860)
- Staged `llama-server` SHA-256:
  `804c1a16fb94be853bf6f416fdad28a5310707ae91daacddef6d13730268ed37`
- Staged size: `12569152` bytes
- Version output: `version: 1 (fdb1db8)`, AppleClang 17, Darwin arm64
- CPU compatibility fixtures: passed
- Metal compatibility fixtures: passed
- Non-system dynamic dependencies: none

The build and verification command was:

```sh
cd app
pnpm release:build:app
```

The generated sidecar, license, and release manifest are intentionally ignored build artifacts.
The release manifest travels inside the resulting `.app` and records the exact artifact hash for
that build.

## Packaged `.app`

`pnpm release:verify-app` confirmed:

- an ad-hoc-signed arm64 app with identifier `com.silo77.dara`;
- strict deep signature verification for the complete bundle;
- a minimum system version of macOS 14.0;
- the executable bit and staged SHA-256 on `Contents/Resources/bin/llama-server`;
- only system frameworks and `/usr/lib` dependencies for the sidecar;
- exact embedding manifest, golden fixture, release manifest, and llama.cpp license resources;
- no GGUF, browser/native test, Playwright, WDIO, or E2E resource.

The public distribution path is a separate, stricter superset of these gates.
`pnpm release:build:distribution` first verifies the exact unsigned sidecar
hashes above, then signs the app and both sidecars with SILO77's Developer ID
identity and hardened runtime. Because an Apple signature changes the Mach-O
bytes, the installed Litestream runtime accepts either the exact upstream hash
or its exact Dara sidecar identifier signed by the pinned SILO77 certificate
and team. The command notarizes and staples both the app and final DMG, mounts
the DMG, and runs the package, stapler, and Gatekeeper checks against the copy
users will install.

The distribution command owns Apple submission polling rather than delegating
an uninterruptible wait to Tauri. It records each submission ID and exact
upload SHA-256 beneath the ignored release bundle directory. The application
submission state also records the exact signed sidecar hashes, so resuming and
final artifact verification do not depend on the temporary signing-staging
directory still existing. Transient status and stapling failures are retried,
and `pnpm release:resume:distribution` continues those saved submissions
without rebuilding or uploading again.

## Isolated runtime smoke

The packaged app was launched against
`app/.data/release-sidecar-smoke-20260726-2248/` with deliberately invalid
`DARA_LLAMA_SERVER_PATH` and `DARA_EMBEDDING_MODEL_PATH` values. The release ignored both
development overrides and wrote a valid verification receipt naming:

- the model below that repository-local data directory; and
- `Dara.app/Contents/Resources/bin/llama-server` as the sidecar.

The bundled sidecar loaded the model, passed the query and document fixtures, and served the local
embedding endpoint. This smoke used a process signal for test cleanup, so it does not claim the
normal Cmd+Q lifecycle check; that remains part of clean-first-run acceptance.
