# Agent notes

## Using Canvas
Use the Canvas LMS API when the user asks about courses, assignments,
announcements, grades, or submissions.

Environment variables (for scripts and agents working outside the app):
- CANVAS_BASE_URL
- CANVAS_ACCESS_TOKEN

Never print or expose CANVAS_ACCESS_TOKEN.

## Working on this repository
- UI: `src/` (React + TypeScript). Backend: `src-tauri/src/` (Rust, Tauri 2).
- Run `npm test` and `cargo test --manifest-path src-tauri/Cargo.toml` before committing.
- Keep the Codex bridge read-only: new Codex methods must be added to `validated_request` in `codex.rs` with fixed parameters.
- AI-initiated Canvas changes go through the approval queue (`propose_` tools → `pending_actions` → `pending_decide`). The MCP helper must never call `execute_write`.
- User-initiated changes from the app UI use `canvas_action` after a confirmation dialog in the UI. The AI never chooses local files to upload; the user attaches them.
- Chat attachments are copied into `<app data>/attachments/<id>/`; the AI reads them only by id (`read_attachment`), and image input paths are resolved from ids in `CodexState::call`, never taken from the UI.
- Files the AI makes (`make_file`) go only to `Downloads/Canvas Assistant/AI 생성`; executables are refused and nothing is overwritten.
- Keep versions in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` in sync.
- UI copy is Korean.
