# Repository guidance

## Git staging, commits, and pushes

- Never run `git add`, `git commit`, or `git push` without first receiving the user's explicit approval for those specific actions.
- Do not infer approval from a request to implement a change, fix an issue, address review feedback, prepare a pull request, rebase branches, or otherwise complete a development task.
- Approval applies only to the specific batch of staging, commits, or pushes being discussed. Do not treat prior approval as permission for later Git mutations.
- Without approval, leave changes unstaged and uncommitted, report their status, and provide a suggested commit message if useful.

## Local development database

- Any development work that reads from or writes to a Dara database must use a repository-local testing database under `app/.data/`. This includes manual SQL inspection, migration work, smoke tests, benchmarks, development scripts, and direct application launches.
- Use `app/.data/local/` by default. Running `pnpm tauri dev` from `app/` already sets `DARA_DATA_DIR="$PWD/.data/local"`. When invoking Cargo, the Dara binary, or another database-aware tool directly, set `DARA_DATA_DIR` explicitly to `app/.data/local` or to an isolated task-specific directory beneath `app/.data/`.
- Do not allow development commands to fall back to Dara's platform data directory or otherwise read or mutate a non-testing user database. Automated tests may continue to use their isolated temporary database directories.
- Treat existing files under `app/.data/` as test data that may still be useful to the developer: do not delete, reset, or replace them unless the user explicitly requests it. For destructive migration experiments, first copy the database pair to a separate task-specific directory under `app/.data/` and work on the copy.

## Release distribution and validation

- Use Dara's built-in Tauri updater for normal production upgrades. Build, sign, notarize, and verify the release from clean `main`; create and push the annotated version tag; upload and verify the updater assets in a draft GitHub Release; then publish that release. Publishing the GitHub Release, not merely pushing the tag, exposes `latest.json` to installed applications.
- Do not manually replace `/Applications/Dara.app` with a release candidate or run a pre-publication smoke check against the production Dara database. After publication, the user validates the real upgrade path by accepting Dara's in-app update notification from the previously installed production version.
- Skip the manual repository-local packaged release acceptance suite by default. Run any part of that suite only when the user explicitly requests it for the specific release.
- Do not launch a release candidate against the production data directory or otherwise read or modify production Dara data as part of release automation.

## UI consistency and existing patterns

- Before adding or styling an interactive control, search the repository for existing controls with the same role or interaction model. Inspect their component code, styles, states, keyboard behavior, and tests. The closest established Dara control is the default design reference; do not treat a new feature as a blank-slate styling exercise.
- Reuse an existing component when its semantics fit. When multiple features need the same interaction, extract or extend a shared primitive instead of copying its CSS or creating a feature-local approximation. Feature components should usually supply labels, options, and domain behavior while the shared primitive owns presentation and interaction.
- Do not introduce a platform-default browser control merely because it is convenient when Dara already has an app-owned equivalent. In particular, a new dropdown should reuse or generalize the established trigger + popover/listbox pattern rather than adding a native `<select>` with unrelated appearance and behavior.
- Native controls remain appropriate when their native semantics or platform integration are intentional requirements. If an established Dara pattern cannot be reused, state the concrete reason before implementation and keep the new control aligned with the existing visual tokens and interaction conventions.
- Review a new control beside its closest existing analogue before considering the work complete. Verify at least its normal, hover, focus, open/active, disabled, and dark-mode states as applicable, along with keyboard navigation and focus return. Add behavior tests for shared primitives; when visual prior art exists, explicitly compare against it rather than judging the new control in isolation.

## Closed string sets

- Represent every finite domain, state-machine, protocol, lifecycle, or UI-mode string set with named values. Do not scatter raw string literals through comparisons, constructors, switches, database queries, or tests.
- In TypeScript, this project enables `erasableSyntaxOnly`, so use an `as const` value object plus a derived union type instead of the `enum` keyword:

  ```ts
  export const ReviewPhase = {
    Question: 'QUESTION',
    CaughtUp: 'CAUGHT_UP',
  } as const

  export type ReviewPhase =
    (typeof ReviewPhase)[keyof typeof ReviewPhase]
  ```

  Consume `ReviewPhase.Question`, including in tests and fixtures assembled in code.
- In Rust, use native enums. Apply Serde renaming at API boundaries and centralize any database/wire conversion in the enum implementation. Database reads, writes, query parameters, application logic, and test seed helpers must use the enum rather than repeating its persisted strings.
- Keep each serialized value in one authoritative mapping. Adding a variant must produce compiler-guided updates through exhaustive TypeScript/Rust branches and boundary conversions.
- Literal strings remain appropriate when they are external grammar rather than Dara-owned closed sets—for example CSS classes, DOM roles, browser key names, Markdown/ProseMirror node names, SQL schema and migration syntax, or verbatim golden payloads. Use named constants for singleton identifiers and pinned versions.
