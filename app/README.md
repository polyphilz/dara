# dara desktop

The macOS desktop client: React + TypeScript + Vite inside Tauri v2.

## Development

```bash
pnpm install
pnpm tauri dev
```

The application launches as a menu-bar resident app with both native windows hidden. Development
data is isolated under `.data/local/`, including `dara.sqlite3`, `media.sqlite3`, and backups.
Installed builds continue to use the platform application-data directory.

## Shortcuts

- `⌃⌥⌘D` activates the frameless Quick Add window while Dara remains in menu-bar-only Accessory mode.
- `⌃⌥⌘R` activates dara and opens the ordinary main window.
- `Esc` cancels quick add; `⌘↵` saves a BASIC card.
- `⌘B`, `⌘I`, and `⌘K` apply bold, italic, and link formatting in card editors.

`Add card` in the main window opens a persistent rich-text creation view in that window. Quick Add
is reserved for global capture over another application. Both surfaces serialize formatted content
to Dara Markdown when saving; users do not edit Markdown punctuation directly. Save or Cancel
hides Quick Add and restores the previously active application. Dara appears in the Dock and owns
the application menu only while its ordinary main window is open.

## Checks

```bash
pnpm test
pnpm lint
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build --debug --no-bundle
```
