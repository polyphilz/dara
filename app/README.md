# dara desktop

The macOS desktop client: React + TypeScript + Vite inside Tauri v2.

## Development

```bash
pnpm install
pnpm tauri dev
```

The application launches as a menu-bar resident app with both native windows hidden.

## Activation spike

- `⌃⌥⌘D` opens the non-activating quick-add panel.
- `⌃⌥⌘R` activates dara and opens the ordinary main window.
- `Esc` cancels quick add; `⌘↵` exercises its temporary save path.

The spike deliberately persists nothing. Its purpose is to validate keyboard input and exact
focus restoration across applications, Spaces, fullscreen windows, monitors, and IME input
before product implementation begins.

## Checks

```bash
pnpm lint
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build --debug --no-bundle
```
