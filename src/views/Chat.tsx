import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { api, errorText, Model } from "../api";
import { useApp } from "../App";
import { Banner, Markdown, Spinner } from "../components";
import CodexLogin from "../CodexLogin";
import ToolPicker from "../ToolPicker";
import { composeMessage, splitMessage } from "../toolkit";
import { attachBytes, attachPaths, describeRef, kindLabel, pickFiles } from "../shared";
import { bytes, effortLabels, toolLabels } from "../util";

type Entry =
  | { kind: "user"; id: string; text: string; turnId?: string; images?: string[]; steered?: boolean }
  | { kind: "assistant"; id: string; text: string; done: boolean }
  | { kind: "tool"; id: string; tool: string; server: string; status: "running" | "done" | "error"; detail?: string; file?: MadeFile };

type MadeFile = { name: string; path: string; size?: number };

type Thread = { id: string; name?: string | null; preview?: string; updatedAt?: number };

const suggestions = [
  "이번 주 마감인 과제를 마감 순서대로 정리해줘",
  "최근 1주일 공지 중 중요한 것만 요약해줘",
  "과목별 현재 성적을 표로 보여줘",
  "다음 주 일정이랑 할 일을 계획표로 만들어줘",
];

function itemToEntry(item: any): Entry | null {
  switch (item?.type) {
    case "userMessage": {
      const content: any[] = item.content ?? [];
      return {
        kind: "user", id: item.id,
        text: content.filter((c) => c.type === "text").map((c) => c.text ?? "").join("\n"),
        images: content.filter((c) => c.type === "localImage").map((c) => attachmentId(String(c.path ?? ""))).filter((x): x is string => !!x),
      };
    }
    case "agentMessage":
      return { kind: "assistant", id: item.id, text: item.text ?? "", done: true };
    case "mcpToolCall":
      return {
        kind: "tool", id: item.id, tool: item.tool, server: item.server,
        status: item.status === "failed" || item.error ? "error" : item.status === "inProgress" ? "running" : "done",
        detail: toolDetail(item),
        file: item.tool === "make_file" ? madeFile(item) : undefined,
      };
    default:
      return null;
  }
}

/** Attachment id from a stored image path (…/attachments/<id>/<name>). */
function attachmentId(path: string): string | undefined {
  return /attachments[\\/]([0-9a-f]{20})[\\/]/.exec(path)?.[1];
}

function fileToBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).replace(/^data:[^,]*,/, ""));
    r.onerror = () => reject(r.error);
    r.readAsDataURL(file);
  });
}

/** Error text of a failed tool call, so the user sees why instead of only the AI's summary. */
function toolDetail(item: any): string | undefined {
  if (item?.error?.message) return String(item.error.message);
  if (item?.status !== "failed") return undefined;
  const text = (item?.result?.content ?? []).map((c: any) => c?.text ?? "").join("\n").trim();
  return text ? text.slice(0, 600) : undefined;
}

/** The saved file reported by the make_file tool. */
function madeFile(item: any): MadeFile | undefined {
  if (item?.status === "failed") return undefined;
  for (const c of item?.result?.content ?? []) {
    try {
      const v = JSON.parse(c?.text ?? "");
      if (v?.saved && v.path) return { name: String(v.name ?? ""), path: String(v.path), size: Number(v.size) || undefined };
    } catch { /* not JSON */ }
  }
  return undefined;
}

function FileCard({ file }: { file: MadeFile }) {
  const [msg, setMsg] = useState("");
  const run = async (reveal: boolean) => {
    setMsg("");
    try { await invoke("open_generated", { path: file.path, reveal }); } catch (e) { setMsg(errorText(e)); }
  };
  return (
    <div className="file-card">
      <span className="file-icon" aria-hidden>{fileIcon(file.name)}</span>
      <div className="grow">
        <div className="title">{file.name}</div>
        <div className="muted small">{file.size != null ? `${bytes(file.size)} · ` : ""}다운로드 › Canvas Assistant › AI 생성</div>
        {msg && <div className="error-text small">{msg}</div>}
      </div>
      <button onClick={() => void run(false)}>열기</button>
      <button onClick={() => void run(true)}>폴더에서 보기</button>
    </div>
  );
}

function fileIcon(name: string) {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "pdf") return "📕";
  if (ext === "docx") return "📘";
  if (ext === "zip") return "🗜";
  if (["csv", "tsv"].includes(ext)) return "📊";
  if (["md", "txt"].includes(ext)) return "📝";
  return "📄";
}

export default function Chat({ prefill }: { prefill: { text: string; n: number; send?: boolean } | null }) {
  const { codex, info, refreshInfo, go, attachments, addAttachment, removeAttachment, clearAttachments } = useApp();
  const [dragging, setDragging] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [picker, setPicker] = useState(false);
  const [models, setModels] = useState<Model[]>([]);
  const [model, setModel] = useState("");
  const [effort, setEffort] = useState("");
  const [threads, setThreads] = useState<Thread[]>([]);
  const [threadId, setThreadId] = useState("");
  const [turnId, setTurnId] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const threadRef = useRef("");
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const ready = codex.state === "ready" && !!codex.account;

  useEffect(() => { setSlot(document.getElementById("sidebar-slot")); }, []);
  useEffect(() => { threadRef.current = threadId; }, [threadId]);
  const pendingSend = useRef<number | null>(null);
  useEffect(() => {
    if (!prefill) return;
    setDraft(prefill.text);
    if (prefill.send) pendingSend.current = prefill.n;
  }, [prefill]);

  const loadThreads = useCallback(async () => {
    try {
      const r = await api.codex<{ data: Thread[] }>("thread/list");
      let list = r.data ?? [];
      if (list.length === 0) {
        // Fallback when the workspace path filter does not match how Codex stored it.
        const all = await api.codex<{ data: Array<Thread & { originator?: string }> }>("thread/list", { scope: "byOriginator" });
        list = (all.data ?? []).filter((t) => t.originator === "canvas_assistant");
      }
      setThreads(list);
    } catch { /* ignore */ }
  }, []);

  // Models and threads once Codex is signed in.
  useEffect(() => {
    if (!ready) return;
    void loadThreads();
    void (async () => {
      try {
        const r = await api.codex<{ data: Model[] }>("model/list");
        const list = r.data ?? [];
        setModels(list);
        const saved = info?.settings.defaultModel;
        const pick = list.find((m) => m.model === saved) ?? list.find((m) => m.isDefault) ?? list[0];
        if (pick) {
          setModel(pick.model);
          const savedEffort = info?.settings.defaultEffort;
          const efforts = pick.supportedReasoningEfforts?.map((e) => e.reasoningEffort) ?? [];
          setEffort(savedEffort && efforts.includes(savedEffort) ? savedEffort : pick.defaultReasoningEffort ?? "");
        }
      } catch (e) { setError(errorText(e)); }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, loadThreads]);

  // Streaming events for the open thread.
  useEffect(() => {
    const un = listen<{ method: string; params: any }>("codex-event", ({ payload }) => {
      const { method, params } = payload;
      if (params?.threadId && params.threadId !== threadRef.current) {
        if (method === "thread/name/updated" || method === "turn/completed") void loadThreads();
        return;
      }
      switch (method) {
        case "turn/started": {
          const tid = params?.turn?.id ?? "";
          setTurnId(tid);
          setBusy(true);
          if (tid) setEntries((old) => old.map((e) => (e.kind === "user" && !e.turnId ? { ...e, turnId: tid } : e)));
          break;
        }
        case "item/agentMessage/delta":
          setEntries((old) => {
            const i = old.findIndex((e) => e.kind === "assistant" && e.id === params.itemId);
            if (i === -1) return [...old, { kind: "assistant", id: params.itemId, text: params.delta ?? "", done: false }];
            const next = old.slice();
            const cur = next[i] as Extract<Entry, { kind: "assistant" }>;
            next[i] = { ...cur, text: cur.text + (params.delta ?? "") };
            return next;
          });
          break;
        case "item/started":
        case "item/completed": {
          const entry = itemToEntry(params?.item);
          if (!entry || entry.kind === "user") break;
          const started = method === "item/started";
          if (entry.kind === "tool" && started) entry.status = "running";
          if (entry.kind === "assistant" && started) entry.done = false;
          setEntries((old) => {
            const i = old.findIndex((e) => e.id === entry.id);
            if (i === -1) return [...old, entry];
            if (started) return old; // deltas may already have arrived
            const next = old.slice();
            next[i] = entry;
            return next;
          });
          break;
        }
        case "turn/completed": {
          setBusy(false);
          setTurnId("");
          const t = params?.turn;
          if (t?.status === "failed" && t?.error?.message) setError(t.error.message);
          setEntries((old) => old.map((e) => (e.kind === "assistant" ? { ...e, done: true } : e)));
          void loadThreads();
          void refreshInfo();
          break;
        }
        case "mcpServer/startupStatus/updated":
          if (params?.name === "canvas" && (params?.status === "failed" || params?.error)) {
            setError(`Canvas 도구를 시작하지 못했습니다: ${params?.error ?? params?.failureReason ?? params?.status}. 설정 → 'AI의 Canvas 도구 점검'으로 확인하세요.`);
          }
          break;
        case "error":
          if (!params?.willRetry) setError(params?.error?.message ?? "Codex 오류가 발생했습니다.");
          break;
        case "thread/name/updated":
          void loadThreads();
          break;
      }
    });
    return () => { void un.then((f) => f()); };
  }, [loadThreads, refreshInfo]);

  useEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [entries]);

  function newChat() {
    setThreadId("");
    setEntries([]);
    setError("");
    setBusy(false);
  }

  async function openThread(id: string) {
    if (id === threadId) return;
    setError("");
    setEntries([]);
    setBusy(false);
    setThreadId(id);
    try {
      await api.codex("thread/resume", { threadId: id });
      const r = await api.codex<{ data: any[] }>("thread/turns/list", { threadId: id });
      const list: Entry[] = [];
      for (const turn of [...(r.data ?? [])].reverse()) for (const item of turn.items ?? []) {
        const e = itemToEntry(item);
        if (e?.kind === "user") e.turnId = turn.id;
        if (e) list.push(e);
      }
      if (threadRef.current === id) setEntries(list);
    } catch (e) { setError(errorText(e)); }
  }

  /** `fromComposer: false` resends an edited message as-is, without the composer's pending attachments. */
  async function send(textArg?: string, extraImages: string[] = [], fromComposer = true) {
    const pending = fromComposer ? attachments : [];
    const typed = (textArg ?? draft).trim();
    if ((!typed && pending.length === 0 && extraImages.length === 0) || !ready) return;
    const steering = busy && !!turnId && !!threadId;
    if (busy && !steering) return;
    const text = fromComposer ? composeMessage(typed, pending) : typed;
    const images = [...extraImages, ...pending.flatMap((a) => (a.type === "file" && a.file.image ? [a.file.id] : []))];
    setBusy(true);
    setError("");
    if (fromComposer) {
      setDraft("");
      clearAttachments();
    }
    stick.current = true;
    const localId = `local-${Date.now()}`;
    setEntries((old) => [...old, { kind: "user", id: localId, text, images, steered: steering, turnId: steering ? turnId : undefined }]);
    try {
      if (steering) {
        try {
          await api.codex("turn/steer", { threadId, expectedTurnId: turnId, text, images });
          return;
        } catch {
          // The turn ended in the meantime: send it as a new turn instead.
          setEntries((old) => old.map((e) => (e.id === localId && e.kind === "user" ? { ...e, steered: false, turnId: undefined } : e)));
        }
      }
      let id = threadId;
      if (!id) {
        const r = await api.codex<{ thread: { id: string } }>("thread/start", { model });
        id = r.thread?.id;
        if (!id) throw new Error("Codex가 대화 ID를 돌려주지 않았습니다.");
        setThreadId(id);
        threadRef.current = id;
      }
      const r = await api.codex<{ turn?: { id: string } }>("turn/start", { threadId: id, text, model, effort, images });
      const tid = r?.turn?.id;
      if (tid) {
        setTurnId((cur) => cur || tid);
        setEntries((old) => old.map((e) => (e.id === localId && e.kind === "user" ? { ...e, turnId: tid } : e)));
      }
    } catch (e) {
      setBusy(false);
      setError(errorText(e));
    }
  }

  /** Edit an earlier message: drop its turn and everything after it, then send the new text. */
  async function editMessage(entry: Extract<Entry, { kind: "user" }>, main: string) {
    setEditing(null);
    if (!threadId || !entry.turnId || busy) return;
    const { context } = splitMessage(entry.text);
    const text = main.trim() + (context ? `\n\n${context}` : "");
    if (!main.trim() && !context) return;
    try {
      await api.codex("thread/revert", { threadId, beforeTurnId: entry.turnId });
      const index = entries.findIndex((e) => e.kind === "user" && e.turnId === entry.turnId);
      setEntries((old) => old.slice(0, index === -1 ? old.length : index));
      await send(text, entry.images ?? [], false);
    } catch (e) {
      setError(`메시지를 수정하지 못했습니다: ${errorText(e)}`);
    }
  }

  async function attachFiles(paths: string[]) {
    if (!paths.length) return;
    try {
      for (const f of await attachPaths(paths)) addAttachment({ type: "file", key: `file:${f.id}`, file: f });
    } catch (e) { setError(errorText(e)); }
  }

  async function onPaste(e: React.ClipboardEvent<HTMLTextAreaElement>) {
    const files = Array.from(e.clipboardData?.files ?? []);
    if (!files.length) return;
    e.preventDefault();
    for (const f of files.slice(0, 10)) {
      try {
        const name = f.name && f.name !== "image.png" ? f.name : `붙여넣은 이미지 ${new Date().toLocaleTimeString("ko-KR", { hour12: false }).replace(/:/g, "")}.${(f.type.split("/")[1] || "png").replace("jpeg", "jpg")}`;
        const saved = await attachBytes(name, await fileToBase64(f));
        addAttachment({ type: "file", key: `file:${saved.id}`, file: saved });
      } catch (err) { setError(errorText(err)); }
    }
  }

  // Files dropped onto the window (Tauri gives real paths).
  useEffect(() => {
    let un: (() => void) | undefined;
    let alive = true;
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((ev) => {
      const t = ev.payload.type;
      if (t === "enter" || t === "over") setDragging(true);
      else if (t === "leave") setDragging(false);
      else if (t === "drop") { setDragging(false); void attachFiles(ev.payload.paths ?? []); }
    })).then((f) => { if (alive) un = f; else f(); }).catch(() => undefined);
    return () => { alive = false; un?.(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // "AI로 바로 분석" from a right-click menu sends as soon as the chat is ready.
  useEffect(() => {
    if (pendingSend.current != null && ready && !busy && draft) {
      pendingSend.current = null;
      void send(draft);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, busy, draft, attachments]);

  async function interrupt() {
    if (!threadId || !turnId) return;
    try { await api.codex("turn/interrupt", { threadId, turnId }); } catch (e) { setError(errorText(e)); }
  }


  async function saveModel(m: string, e: string) {
    try { await api.saveSettings({ defaultModel: m, defaultEffort: e }); } catch { /* not critical */ }
  }

  async function rename(id: string, name: string) {
    setRenaming(null);
    if (!name.trim()) return;
    try { await api.codex("thread/name/set", { threadId: id, name }); await loadThreads(); } catch (e) { setError(errorText(e)); }
  }

  async function archive(id: string) {
    if (!window.confirm("이 대화를 목록에서 보관할까요?")) return;
    try {
      await api.codex("thread/archive", { threadId: id });
      if (id === threadId) newChat();
      await loadThreads();
    } catch (e) { setError(errorText(e)); }
  }

  async function remove(id: string) {
    if (!window.confirm("이 대화를 삭제할까요? 대화 기록이 PC에서도 지워지고 되돌릴 수 없습니다.")) return;
    try {
      await api.codex("thread/delete", { threadId: id });
      if (id === threadId) newChat();
      await loadThreads();
    } catch (e) { setError(errorText(e)); }
  }

  const current = models.find((m) => m.model === model);
  const efforts = useMemo(() => current?.supportedReasoningEfforts?.map((e) => e.reasoningEffort) ?? [], [current]);
  const canvasReady = !!(info?.hasToken && info.settings.canvasBaseUrl);

  const threadList = slot && createPortal(
    <div className="threads">
      <button className="new-chat" onClick={newChat}>＋ 새 대화</button>
      <div className="subhead">최근 대화</div>
      <div className="thread-scroll">
        {threads.length === 0 && <div className="muted small pad">아직 대화가 없습니다.</div>}
        {threads.map((t) => (
          <div key={t.id} className={"thread" + (t.id === threadId ? " active" : "")}>
            {renaming === t.id ? (
              <input autoFocus defaultValue={t.name ?? t.preview ?? ""} maxLength={80}
                onBlur={(e) => void rename(t.id, e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") void rename(t.id, (e.target as HTMLInputElement).value); if (e.key === "Escape") setRenaming(null); }} />
            ) : (
              <button className="thread-title" onClick={() => void openThread(t.id)} title={t.name ?? t.preview}>{t.name || t.preview || "새 대화"}</button>
            )}
            <span className="thread-actions">
              <button className="ghost icon" title="이름 바꾸기" onClick={() => setRenaming(t.id)}>✎</button>
              <button className="ghost icon" title="보관" onClick={() => void archive(t.id)}>🗄</button>
              <button className="ghost icon danger" title="삭제" onClick={() => void remove(t.id)}>🗑</button>
            </span>
          </div>
        ))}
      </div>
    </div>,
    slot,
  );

  return (
    <div className="chat">
      {threadList}
      <header className="topbar">
        <h1>AI 대화</h1>
        {ready && models.length > 0 && (
          <div className="model-picker">
            <select aria-label="모델" value={model} onChange={(e) => {
              const m = models.find((x) => x.model === e.target.value);
              setModel(e.target.value);
              const eff = m?.defaultReasoningEffort ?? "";
              setEffort(eff);
              void saveModel(e.target.value, eff);
            }}>
              {models.map((m) => <option key={m.id} value={m.model}>{m.displayName ?? m.model}</option>)}
            </select>
            {efforts.length > 0 && (
              <select aria-label="추론 수준" value={effort} onChange={(e) => { setEffort(e.target.value); void saveModel(model, e.target.value); }}>
                {efforts.map((v) => <option key={v} value={v}>추론: {effortLabels[v] ?? v}</option>)}
              </select>
            )}
          </div>
        )}
      </header>

      {error && <Banner onClose={() => setError("")}>{error}</Banner>}

      <div className="conversation" ref={scroller} onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}>
        {codex.state === "starting" && <div className="center"><Spinner label="Codex 준비 중…" /></div>}
        {(codex.state === "error" || (codex.state === "ready" && !codex.account)) && (
          <div className="card setup">
            <h2>{codex.state === "error" ? "Codex에 연결하지 못했습니다" : "ChatGPT 계정으로 로그인"}</h2>
            <CodexLogin />
          </div>
        )}
        {ready && entries.length === 0 && (
          <div className="welcome">
            <img src="/logo.png" alt="" />
            <h2>무엇을 도와드릴까요?</h2>
            <p className="muted">{canvasReady ? "Canvas의 과제·공지·성적을 직접 조회해서 답합니다." : "Canvas를 연결하면 과제·공지·성적을 직접 조회해서 답할 수 있습니다."}</p>
            {!canvasReady && <button onClick={() => go("settings")}>Canvas 연결하기</button>}
            {canvasReady && (
              <div className="suggestions">
                {suggestions.map((s) => <button key={s} onClick={() => void send(s)}>{s}</button>)}
              </div>
            )}
          </div>
        )}
        {entries.map((e) =>
          e.kind === "user" ? (
            <UserBubble key={e.id} entry={e} editing={editing === e.id} canEdit={!!e.turnId && !busy && !e.steered}
              onEdit={() => setEditing(e.id)} onCancel={() => setEditing(null)} onSave={(t) => void editMessage(e, t)} />
          ) : e.kind === "assistant" ? (
            <div key={e.id} className="msg assistant">
              <Markdown text={e.text} />
              {!e.done && <span className="cursor" aria-hidden />}
            </div>
          ) : (
            e.file ? <FileCard key={e.id} file={e.file} /> :
            <div key={e.id} className={"tool-chip " + e.status}>
              {e.status === "running" ? <span className="spinner" aria-hidden /> : <span aria-hidden>{e.status === "error" ? "⚠" : "✓"}</span>}
              {e.server === "canvas" ? "Canvas · " : `${e.server} · `}{toolLabels[e.tool] ?? e.tool}
              {e.tool.startsWith("propose_") && e.status === "done" && <button className="link" onClick={() => go("approvals")}>승인하러 가기 →</button>}
              {e.status === "error" && e.detail && <details className="tool-error"><summary>오류 보기</summary><span>{e.detail}</span></details>}
            </div>
          ),
        )}
        {busy && !entries.some((e) => (e.kind === "assistant" && !e.done) || (e.kind === "tool" && e.status === "running")) && (
          <div className="msg assistant"><Spinner label="생각하는 중…" /></div>
        )}
      </div>

      {attachments.length > 0 && (
        <div className="attach-row" aria-label="첨부">
          {attachments.map((a) => (
            <span key={a.key} className={"attach-chip" + (a.type === "tool" ? " tool" : "")} title={a.type === "ref" ? describeRef(a.ref) : a.type === "file" ? `${a.file.name} (${bytes(a.file.size)})` : a.tool}>
              {a.type === "ref" ? `${kindLabel[a.ref.kind]} · ${a.ref.label}` : a.type === "file" ? `${a.file.image ? "🖼" : "📎"} ${a.file.name}` : (a.mode === "result" ? "🧰 " : "🤖 ") + a.label}
              <button className="ghost icon" aria-label="첨부 빼기" onClick={() => removeAttachment(a.key)}>✕</button>
            </span>
          ))}
        </div>
      )}
      <div className={"composer" + (dragging ? " dragging" : "")}>
        {dragging && <div className="drop-hint" aria-hidden>여기에 놓으면 첨부합니다</div>}
        <div className="composer-tools">
          <button className="tool-btn" onClick={async () => { try { await attachFiles((await pickFiles(null, "채팅에 첨부할 파일")).map((f) => f.path)); } catch (e) { setError(errorText(e)); } }}
            title="파일 첨부 (PDF·문서·코드·이미지, 끌어다 놓기·붙여넣기도 됨)" aria-label="파일 첨부" disabled={!ready}>📎</button>
          <button className="tool-btn" onClick={() => setPicker(true)} title="Canvas 도구 불러오기" aria-label="Canvas 도구" disabled={!canvasReady}>🧰</button>
        </div>
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onPaste={(e) => void onPaste(e)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send(); } }}
          placeholder={!ready ? "Codex 로그인 후 사용할 수 있습니다" : busy ? "작업 중에도 보낼 수 있어요 — 보낸 내용은 지금 작업에 바로 반영됩니다" : "메시지를 입력하세요 (Shift+Enter 줄바꿈, 파일은 끌어다 놓기·붙여넣기)"}
          disabled={!ready}
          rows={2}
        />
        {busy && turnId && <button className="stop" onClick={() => void interrupt()} title="응답 중단">■</button>}
        <button className="send primary" onClick={() => void send()} disabled={!ready || (!draft.trim() && attachments.length === 0) || (busy && !turnId)}
          title={busy ? "작업 중인 AI에게 추가로 보내기" : "보내기"}>↑</button>
      </div>
      {picker && <ToolPicker onClose={() => setPicker(false)} />}
    </div>
  );
}

function UserBubble({ entry, editing, canEdit, onEdit, onCancel, onSave }: {
  entry: Extract<Entry, { kind: "user" }>; editing: boolean; canEdit: boolean;
  onEdit: () => void; onCancel: () => void; onSave: (text: string) => void;
}) {
  const { main, context } = splitMessage(entry.text);
  const [value, setValue] = useState(main);
  useEffect(() => { if (editing) setValue(main); }, [editing, main]);
  const count = context ? (context.match(/^- |^\[도구 결과/gm) ?? []).length : 0;
  if (editing) {
    return (
      <div className="msg user editing">
        <div className="bubble edit-box">
          <textarea autoFocus value={value} onChange={(e) => setValue(e.target.value)} rows={Math.min(10, Math.max(2, value.split("\n").length))}
            onKeyDown={(e) => { if (e.key === "Escape") onCancel(); if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); onSave(value); } }} />
          <p className="muted small">보내면 이 메시지부터 아래 대화가 지워지고 새로 답합니다. 첨부는 그대로 유지됩니다.</p>
          <div className="row end">
            <button onClick={onCancel}>취소</button>
            <button className="primary" onClick={() => onSave(value)} disabled={!value.trim() && !context}>보내기</button>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="msg user">
      {canEdit && <button className="ghost icon edit-msg" title="메시지 수정" aria-label="메시지 수정" onClick={onEdit}>✎</button>}
      <div className="bubble">
        {entry.steered && <div className="steer-tag">작업 중 추가</div>}
        {main}
        {(entry.images?.length ?? 0) > 0 && <div className="muted small">🖼 이미지 {entry.images!.length}개</div>}
        {context && (
          <details className="bubble-context">
            <summary>첨부 {count || ""}개 보기</summary>
            <pre className="pre small">{context.length > 4000 ? context.slice(0, 4000) + "…" : context}</pre>
          </details>
        )}
      </div>
    </div>
  );
}
