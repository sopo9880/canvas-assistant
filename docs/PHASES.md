# Canvas Assistant v0.1.0 — scope and status

Windows 10/11 x64 is the only release target. Phases are development steps, not version numbers.

## Phase 1 — Windows shell and Codex ✅
- Tauri 2 + React shell with the graduation-cap AI logo (icons generated from `assets/logo-1024.png`).
- `codex app-server` stdio bridge: method whitelist, read-only sandbox and `approvalPolicy: never` forced on every thread,
  command/file/permission requests always declined, threads scoped to the app's own workspace folder.
- Browser (OAuth) and device-code ChatGPT login, dynamic model/effort picker, streamed Markdown replies,
  history with rename/archive, automatic reconnect, Codex executable discovery for npm installs (`codex.cmd`).

## Phase 2 — Canvas integration ✅
- Canvas base URL validation (https only) and token in Windows Credential Manager; connection verified before saving.
- REST client with pagination (same-origin `Link` only), rate-limit retry, file downloads without leaking the token.
- UI: dashboard (upcoming work, scores, announcements) and per-course assignments/announcements/modules/pages/files/grades/discussions.
- Canvas MCP server (`--mcp-canvas`) registered with Codex through thread config: 13 read tools and 5 `propose_` tools.
- Writes (text/URL submission, discussion reply, planner note, calendar event) only through the approval queue,
  claimed atomically so each request runs at most once; audit log.

## Phase 3 — Windows automation and distribution ✅ (v0.1.0 level)
- SQLite scheduler: deadline reminders, new announcements, grade updates, daily AI briefing; de-duplicated notifications.
- Windows notifications, tray menu, close-to-tray, start with Windows (minimized), single instance.
- Update check against GitHub releases (works once the repository is public; otherwise opens the releases page).
- Linux cross-build script producing the NSIS installer + SHA256 checksums; manual GitHub release workflow.

## Later
- Signed builds and in-app auto-update (needs a public release feed or a separate update endpoint).
- File-upload submissions, quiz support, richer grade analytics.
- Optional bundling of a pinned Codex binary.
- Android is out of scope; Canvas, Codex and scheduler modules stay separate so they can be reused.
