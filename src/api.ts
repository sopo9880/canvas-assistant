import { invoke } from "@tauri-apps/api/core";

export type Settings = {
  canvasBaseUrl: string | null;
  closeToTray: boolean;
  notifications: boolean;
  codexPath: string | null;
  defaultModel: string | null;
  defaultEffort: string | null;
  canvasToolsInChat: boolean;
};

export type AppInfo = {
  version: string;
  /** Rust std::env::consts::OS: "windows", "macos", … */
  platform?: string;
  dataDir: string;
  hasToken: boolean;
  settings: Settings;
  pending: number;
};

export type Job = {
  id: number;
  kind: "deadline_alert" | "new_announcements" | "grade_updates" | "ai_briefing";
  enabled: boolean;
  intervalMin: number;
  params: Record<string, any>;
  lastRun: number | null;
  lastResult: string | null;
};

export type Pending = {
  id: number;
  createdAt: number;
  kind: string;
  summary: string;
  reason: string | null;
  payload: Record<string, any>;
  status: "pending" | "running" | "done" | "failed" | "rejected";
  result: string | null;
  decidedAt: number | null;
};

export type Model = {
  id: string;
  model: string;
  displayName?: string;
  description?: string;
  isDefault?: boolean;
  defaultReasoningEffort?: string;
  supportedReasoningEfforts?: Array<{ reasoningEffort: string; description?: string }>;
};

export const api = {
  info: () => invoke<AppInfo>("app_info"),
  saveSettings: (patch: Partial<Settings>) => invoke<Settings>("settings_save", { patch }),
  checkUpdate: () => invoke<any>("check_update"),
  canvasConnect: (baseUrl: string, token?: string) => invoke<{ baseUrl: string; profile: any }>("canvas_connect", { baseUrl, token: token || null }),
  canvasDisconnect: () => invoke<void>("canvas_disconnect"),
  canvas: <T = any>(tool: string, args: Record<string, unknown> = {}) => invoke<T>("canvas_query", { tool, args }),
  download: (fileId: string) => invoke<string>("canvas_download", { fileId: String(fileId) }),
  lxDownload: (courseId: string, contentId: string) => invoke<string>("learningx_download", { courseId: String(courseId), contentId: String(contentId) }),
  pending: (includeDone = false) => invoke<Pending[]>("pending_list", { includeDone }),
  decide: (id: number, approve: boolean, edits?: Record<string, unknown>) => invoke<any>("pending_decide", { id, approve, edits: edits ?? null }),
  audit: () => invoke<Array<{ at: number; action: string; detail: string; ok: boolean }>>("audit_list"),
  jobs: () => invoke<Job[]>("jobs_list"),
  updateJob: (kind: string, enabled: boolean, intervalMin: number, params: Record<string, unknown>) =>
    invoke<void>("jobs_update", { kind, enabled, intervalMin, params }),
  runJob: (kind: string) => invoke<void>("jobs_run_now", { kind }),
  briefings: () => invoke<Array<{ id: number; at: number; text: string }>>("briefings_list"),
  codexStart: () => invoke<void>("codex_start"),
  codexLocate: () => invoke<{ found: boolean; path?: string; viaNode?: boolean; error?: string; source?: string }>("codex_locate"),
  diagnose: () => invoke<any>("codex_diagnose"),
  codex: <T = any>(method: string, params: Record<string, unknown> = {}) => invoke<T>("codex_call", { method, params }),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as any).message);
  return String(e);
}
