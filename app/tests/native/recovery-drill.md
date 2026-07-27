# Offline recovery drill — 2026-07-26

Status: **passed** on macOS against commit `0ead2a8` plus the protected restore-safety snapshot fix
on `polyphilz/recovery-safety-retention`.

## Scope

The drill used only the repository-local directory:

```text
app/.data/recovery-drill-20260726
```

No platform Dara data directory was read or modified. The fixture began as a copy of the finalized
snapshot `snapshot-1785103931667-16983-0.json`. Its recorded migration heads were main `7` and
media `2`, and `dara recovery verify` accepted the exact pair.

The baseline main/media SHA-256 digests were:

```text
44e67e9ca76f4f6d5a1ddda40be4f1a62630b12e69e4d7ac48d3d20b57403b05  dara.sqlite3
a97fccba771dc84f4f7141d8b64a396b2d4e698b33086cf91d7cbc32e060bb80  media.sqlite3
```

## Baseline evidence

- 18 active card contents: 15 BASIC, 1 CLOZE, and 2 OCCLUSION.
- 5 persisted occlusion masks, 3 image records, and 3 media blobs.
- 7 review events, including 1 revocation.
- 18 search documents.
- SYSTEM appearance, 100% zoom, 2 global keyboard bindings, and 90% desired retention.

## Procedure

1. Built the native executable with `cargo build --locked`.
2. Verified the copied manifest with:

   ```text
   target/debug/dara recovery verify <manifest>
   ```

3. Changed only the drill copy to a recognizable later state by appending the marker
   `RECOVERY_DRILL_PROTECTED_STATE` to one BASIC card and changing zoom from 100% to 150%.
4. Restored the earlier manifest with:

   ```text
   target/debug/dara recovery restore <manifest> <drill-data-directory>
   ```

5. Confirmed before launch that the live database again had the baseline digests, the later-only
   marker was absent, zoom was 100%, both SQLite integrity checks returned `ok`, and the main
   foreign-key check returned no findings.
6. Launched the real Dara debug executable with `DARA_DATA_DIR` set to the drill directory.
7. Opened the main window and checked Browse and Settings:
   - Browse reported all 18 restored cards.
   - Settings reported Dara `0.1.0`, database heads main `7` / media `2`, 100% zoom, and the
     restored shortcuts.
   - Semantic search was Ready with 18 of 18 cards indexed.
   - Media diagnostics reported no missing referenced media.
   - The scheduling check reported: `All 3 reviewed cards are scheduled correctly.`
8. Quit Dara and ran `dara recovery list` and `dara recovery verify` again.

## Restore-safety retention finding

The first real launch exposed that an ordinary pre-restore safety snapshot could be removed by the
automatic daily/weekly/monthly retention pass. The fix gives these snapshots the protected
`restore-safety-` filename class while leaving them in `backups/`, so they remain listable and
restorable by the public recovery commands.

The corrected restore created:

```text
restore-safety-1785118201402-64710-0.json
```

After Dara launched, created its normal launch snapshot, and applied retention:

- the restore intent was removed;
- the rollback directory remained;
- the protected safety manifest and both database files remained;
- `dara recovery list` still reported the safety snapshot; and
- `dara recovery verify` still accepted it.

The retained safety snapshot contained the recognizable later-only marker and 150% zoom, proving
that it represents the pre-restore state rather than the restored live state.

## Automated corroboration

- `cargo test --all-features`: 98 passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `pnpm test:native-bundle-safety`: passed.

