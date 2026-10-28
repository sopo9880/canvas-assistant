import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { api, errorText, Model } from "../api";
import { useApp } from "../App";
import { Banner, Markdown, Spinner } from "../components";
import CodexLogin from "../CodexLogin";
import { effortLabels, toolLabels } from "../util";

type Entry =
  | { kind: "user"; id: string; text: string }
  | { kind: "assistant"; id: string; text: string; done: boolean }
  | { kind: "tool"; id: string; tool: string; server: string; status: "running" | "done" | "error" };

type Thread = { id: string; name?: string | null; preview?: string; updatedAt?: number };

const suggestions = [
  "이번 주 마감인 과제를 마감 순서대로 정리해줘",
  "최근 1주일 공지 중 중요한 것만 요약해줘",
  "과목별 현재 성적을 표로 보여줘",
  "다음 주 일정이랑 할 일을 계획표로 만들어줘",
];

function itemToEntry(item: any): Entry | null {
  switch (item?.type) {
    case "userMessage":
      return { kind: "user", id: item.id, text: (item.content ?? []).map((c: any) => c.text ?? "").join("\n") };
    case "agentMessage":
      return { kind: "assistant", id: item.id, text: item.text ?? "", done: true };
    case "mcpToolCall":
      return { kind: "tool", id: item.id, tool: item.tool, server: item.server, status: item.status === "failed" || item.error ? "error" : item.status === "inProgress" ? "running" : "done" };
    default:
      return null;
  }
}

export default function Chat({ prefill }: { prefill: { text: string; n: number } | null }) {
  const { codex, info, refreshInfo, go } = useApp();
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
  useEffect(() => { if (prefill) setDraft(prefill.text); }, [prefill]);

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
        case "turn/started":
          setTurnId(params?.turn?.id ?? "");
          setBusy(true);
          break;
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
        if (e) list.push(e);
      }
      if (threadRef.current === id) setEntries(list);
    } catch (e) { setError(errorText(e)); }
  }

  async function send(textArg?: string) {
    const text = (textArg ?? draft).trim();
    if (!text || busy || !ready) return;
    setBusy(true);
    setError("");
    setDraft("");
    stick.current = true;
    setEntries((old) => [...old, { kind: "user", id: `local-${Date.now()}`, text }]);
    try {
      let id = threadId;
      if (!id) {
        const r = await api.codex<{ thread: { id: string } }>("thread/start", { model });
        id = r.thread?.id;
        if (!id) throw new Error("Codex가 대화 ID를 돌려주지 않았습니다.");
        setThreadId(id);
        threadRef.current = id;
      }
      await api.codex("turn/start", { threadId: id, text, model, effort });
    } catch (e) {
      setBusy(false);
      setError(errorText(e));
    }
  }

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
            <div key={e.id} className="msg user"><div className="bubble">{e.text}</div></div>
          ) : e.kind === "assistant" ? (
            <div key={e.id} className="msg assistant">
              <Markdown text={e.text} />
              {!e.done && <span className="cursor" aria-hidden />}
            </div>
          ) : (
            <div key={e.id} className={"tool-chip " + e.status}>
              {e.status === "running" ? <span className="spinner" aria-hidden /> : <span aria-hidden>{e.status === "error" ? "⚠" : "✓"}</span>}
              {e.server === "canvas" ? "Canvas · " : `${e.server} · `}{toolLabels[e.tool] ?? e.tool}
              {e.tool.startsWith("propose_") && e.status === "done" && <button className="link" onClick={() => go("approvals")}>승인하러 가기 →</button>}
            </div>
          ),
        )}
        {busy && !entries.some((e) => (e.kind === "assistant" && !e.done) || (e.kind === "tool" && e.status === "running")) && (
          <div className="msg assistant"><Spinner label="생각하는 중…" /></div>
        )}
      </div>

      <div className="composer">
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void send(); } }}
          placeholder={ready ? "메시지를 입력하세요 (Shift+Enter 줄바꿈)" : "Codex 로그인 후 사용할 수 있습니다"}
          disabled={!ready}
          rows={2}
        />
        {busy && turnId ? (
          <button className="stop" onClick={() => void interrupt()} title="응답 중단">■</button>
        ) : (
          <button className="send primary" onClick={() => void send()} disabled={!ready || !draft.trim() || busy} title="보내기">↑</button>
        )}
      </div>
    </div>
  );
}
