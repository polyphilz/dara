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

- `⌃⌥⌘D` opens the non-activating quick-add panel.
- `⌃⌥⌘R` activates dara and opens the ordinary main window.
- `Esc` cancels quick add; `⌘↵` saves a BASIC card.

## Checks

```bash
pnpm lint
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build --debug --no-bundle
```
