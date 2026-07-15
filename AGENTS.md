# Repository guidance

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
