# Repository guidance

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
