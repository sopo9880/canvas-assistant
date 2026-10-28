import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "./api";
import { Badge } from "./components";
import { formatDate, timeLeft } from "./util";

/** A Canvas object the user can right-click, attach to a chat or act on. */
export type CanvasRef = {
  kind: "assignment" | "announcement" | "discussion" | "file" | "page" | "quiz" | "course" | "module_item" | "message" | "planner" | "material";
  label: string;
  courseId?: string;
  courseName?: string;
  id?: string;
  htmlUrl?: string;
  dueAt?: string | null;
  extra?: Record<string, unknown>;
};

export const kindLabel: Record<CanvasRef["kind"], string> = {
  assignment: "과제", announcement: "공지", discussion: "토론", file: "파일", page: "페이지",
  quiz: "퀴즈", course: "강의", module_item: "모듈 항목", message: "메시지", planner: "할 일", material: "강의자료",
};

/** One line describing a reference for the AI, with the ids it needs for tool calls. */
export function describeRef(r: CanvasRef): string {
  const ids: string[] = [];
  if (r.courseId) ids.push(`course_id=${r.courseId}`);
  if (r.id) ids.push(`${r.kind === "assignment" ? "assignment_id" : r.kind === "discussion" || r.kind === "announcement" ? "topic_id" : r.kind === "file" ? "file_id" : r.kind === "quiz" ? "quiz_id" : r.kind === "message" ? "conversation_id" : r.kind === "page" ? "page_url" : r.kind === "material" ? "content_id" : "id"}=${r.id}`);
  for (const [k, v] of Object.entries(r.extra ?? {})) if (v != null && v !== "") ids.push(`${k}=${v}`);
  const parts = [`${kindLabel[r.kind]} "${r.label}"`, `(${ids.join(", ")})`];
  if (r.courseName) parts.push(`강의: ${r.courseName}`);
  if (r.dueAt) parts.push(`마감: ${formatDate(r.dueAt)}`);
  return parts.join(" · ");
}

export function useLoad<T>(fn: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const reload = useCallback(async () => {
    setLoading(true);
    setError("");
    try { setData(await fn()); } catch (e) { setError(errorText(e)); } finally { setLoading(false); }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(() => { void reload(); }, [reload]);
  return { data, error, loading, reload, setData };
}

export function Due({ iso }: { iso?: string | null }) {
  const t = timeLeft(iso);
  if (!t) return <span className="muted small">마감 없음</span>;
  return <span className="due"><Badge tone={t.tone}>{t.label}</Badge><span className="muted small">{formatDate(iso)}</span></span>;
}

/** Runs a Canvas change the user started from the UI. */
export function canvasAction<T = any>(kind: string, params: Record<string, unknown>) {
  return invoke<T>("canvas_action", { kind, params });
}

export type PickedFile = { path: string; name: string; size: number };
export function pickFiles(extensions?: string[] | null) {
  return invoke<PickedFile[]>("pick_files", { extensions: extensions ?? null });
}
