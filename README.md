# dara

A personal spaced-repetition app for macOS. Built to replace Anki after 6–7 years of use — keeping what works (FSRS scheduling, atomic cards) and fixing what doesn't (slow, bloated, and enough friction in card creation that cards don't get made).

## Principles

- **Fast and resident.** Lives in the menu bar (Docker-style), launches instantly, costs ~nothing at idle.
- **Capture in five seconds.** A global hotkey shows an ephemeral quick-add panel over the current workspace and puts the caret in the editor. Save or cancel and keyboard control returns immediately to the app and window you were using; quick-add does not behave like switching to dara's full application.
- **Keyboard-first everywhere.** Reviews, editing, search, occlusion editing — time off the keyboard is the enemy.
- **Local-first.** One relational SQLite database plus one blob-only media database. No server, sync, or required account. Capture, review, and lexical search work immediately offline; semantic search runs locally after a background model download. Optional R2 backups and the AI mistake-explainer are the only networked features.
- **AI is advisory, never generative.** The AI explains *why you got a card wrong* (you tell it your reasoning, it finds the fault in your understanding). It never writes cards and never touches scheduling.
- **No feature bloat.** No decks (one interleaved pool). No tags (search is the organization). Three card types, nothing else.

## What v1 is

- FSRS scheduling (same modern algorithm Anki uses), desired-retention as the single user-facing knob
- Card types: markdown front/back, cloze deletion, image occlusion (N masks → N sibling cards)
- Ephemeral quick-add panel (global hotkey, restores the prior app/window on dismiss) + full app window for reviewing/editing/searching
- Hybrid search: instant lexical (FTS5) as you type, semantic (local text embeddings) on demand
- OCR on pasted images, so text inside screenshots is searchable
- Edit / suspend / unsuspend / delete / undo-last-grade
- AI mistake-explainer (BYOK or `codex exec`)

## What v1 is not

Mobile, sync, decks, tags, AI card generation, a product for other people. UUIDs, tombstones, and append-only history preserve sync-compatible foundations, but no merge protocol or sync guarantee exists. Anki import is specced but optional — a fresh start is the likely path.

## Stack

Tauri v2. TypeScript UI (React + CodeMirror 6) + `ts-fsrs`; Rust layer for data (rusqlite + statically linked `sqlite-vec` + FTS5), the FSRS optimizer (`fsrs-rs`), a llama.cpp sidecar running Jina v5 nano retrieval embeddings, and the macOS panel glue (`tauri-nspanel`). State is two SQLite files (relational data + immutable media blobs) with an append-only review log as the scheduling source of truth.

## Status

Pre-code. Architecture and product decisions are written up in `.plans/` (local, not committed).
