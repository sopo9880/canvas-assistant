# Canvas Assistant

Windows-first Canvas LMS / Codex personal assistant. **v0.1.0 phase 1 in progress.**

## Phase 1
- Tauri 2 / React desktop shell with selected academic AI app logo.
- Codex app-server connection using a locally installed codex CLI on PATH.
- Official device-code login; account status, dynamic models/effort selection.
- Read-only Codex chat with streamed replies and history via Codex threads.
- PR CI verifies frontend, protocol tests and Windows backend compile.

## Local development
Windows 10/11 x64, Node 22+, Rust stable, Visual Studio C++ Build Tools,
WebView2, Python + Pillow, and the official codex CLI are required.

1. Run: python -m pip install pillow
2. Run: python scripts/prepare_icons.py
3. Run: npm install
4. Run: npm run tauri dev

If codex is not installed, the app will display a setup error.

Canvas API, secure credentials, MCP integration, scheduling, tray, auto-start,
automatic updater, standalone Codex bundling and official EXE release
are **not yet implemented** in this phase. This PR never issues a release or APK.

Codex owns its login tokens. The bridge restricts chat to read-only
and refuses arbitrary RPC and unapproved tool operations. Do not commit tokens.

See docs/PHASES.md for the approved v0.1.0 plan.
