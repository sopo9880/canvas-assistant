# Canvas Assistant v0.1.0 — approved scope

Windows 10/11 x64 is the only release target. These are development phases, not separate version numbers.

## Phase 1 — Windows shell and Codex (this PR)
- Tauri 2 + React shell with selected graduation-cap AI logo.
- Codex app-server stdio bridge and official ChatGPT device-code login.
- Dynamic model/reasoning picker, restricted read-only chat, streamed replies.
- Codex-owned thread history with paging and resume.
- PR checks: frontend unit tests / typecheck and Windows cargo check.

## Phase 2 — Canvas integration (not implemented yet)
- School Canvas base URL validation and Windows credential-store token.
- API registry, pagination and rate-limit handling; courses/assignments/grades/pages/files.
- Permission-gated local MCP Canvas tools for Codex.
- Explicit approval and audit for write operations.

## Phase 3 — Windows automation and distribution (not implemented yet)
- SQLite scheduler, conditional jobs, notifications, Windows autostart and tray.
- HYP/Daglo-inspired signed updater; verify checksums, secure download and safe shutdown.
- CI packaging, signed release metadata, artifact provenance and immutable release distribution.

Android APK, signing and mobile background services are out of scope.
The Codex, Canvas and scheduler modules must remain replaceable for eventual mobile support.

Phase 1 uses an existing local codex executable; fully standalone Codex bundling
requires separately pinning/verifying a Windows Codex binary in a later phase.
A private source repository cannot be used as an unauthenticated public update endpoint.
