import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Banner, openExternal } from "./components";
import { api, AppInfo, errorText } from "./api";
import Chat from "./views/Chat";
import CanvasView from "./views/Canvas";
import Approvals from "./views/Approvals";
import Automation from "./views/Automation";
import SettingsView from "./views/Settings";
import Messages from "./views/Messages";
import { CanvasRef, ChatFile } from "./shared";
import ContextMenu, { MenuState } from "./ContextMenu";
import SubmitDialog from "./SubmitDialog";

export type Tab = "chat" | "canvas" | "messages" | "approvals" | "automation" | "settings";

/** Something attached to the next chat message. */
export type Attachment =
  | { type: "ref"; key: string; ref: CanvasRef }
  | { type: "tool"; key: string; tool: string; label: string; args: Record<string, unknown>; mode: "result" | "instruct"; result?: unknown }
  | { type: "file"; key: string; file: ChatFile };

export type CodexStatus = {
  state: "starting" | "ready" | "error";
  error?: string;
  account?: { type: string; email?: string | null; planType?: string } | null;
};

export type LoginState = {
  mode: "browser" | "device";
  loginId?: string;
  authUrl?: string;
  userCode?: string;
  verificationUrl?: string;
  openFailed?: boolean;
} | null;

type Ctx = {
  info: AppInfo | null;
  login: LoginState;
  loginError: string;
  startLogin: (mode: "browser" | "device") => Promise<void>;
  cancelLogin: () => Promise<void>;
  refreshInfo: () => Promise<void>;
  codex: CodexStatus;
  connectCodex: () => Promise<void>;
  refreshAccount: () => Promise<void>;
  go: (tab: Tab) => void;
  askAI: (prompt: string, opts?: { send?: boolean; refs?: CanvasRef[] }) => void;
  pendingCount: number;
  attachments: Attachment[];
  recentFiles: ChatFile[];
  addAttachment: (a: Attachment) => void;
  removeAttachment: (key: string) => void;
  clearAttachments: () => void;
  openMenu: (e: { clientX: number; clientY: number; preventDefault: () => void }, ref: CanvasRef) => void;
  openSubmit: (ref: CanvasRef) => void;
};

const AppContext = createContext<Ctx | null>(null);
export const useApp = () => useContext(AppContext)!;

export default function App() {
  const [tab, setTab] = useState<Tab>("chat");
  const [newVersion, setNewVersion] = useState<string | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [codex, setCodex] = useState<CodexStatus>({ state: "starting" });
  const [pendingCount, setPendingCount] = useState(0);
  const [prefill, setPrefill] = useState<{ text: string; n: number; send?: boolean } | null>(null);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  // Files attached in chat this session, offered as quick picks on approval cards.
  const [recentFiles, setRecentFiles] = useState<ChatFile[]>([]);
  const [menu, setMenu] = useState<MenuState>(null);
  const [submitRef, setSubmitRef] = useState<CanvasRef | null>(null);
  const retries = useRef(0);
  const [login, setLogin] = useState<LoginState>(null);
  const [loginError, setLoginError] = useState("");
  const loginRef = useRef<LoginState>(null);
  useEffect(() => { loginRef.current = login; }, [login]);

  const refreshInfo = useCallback(async () => {
    try {
      const i = await api.info();
      setInfo(i);
      setPendingCount(i.pending);
    } catch { /* shown elsewhere */ }
  }, []);

  const refreshAccount = useCallback(async () => {
    try {
      const a = await api.codex<{ account: CodexStatus["account"] }>("account/read");
      setCodex({ state: "ready", account: a.account ?? null });
    } catch (e) {
      setCodex({ state: "error", error: errorText(e) });
    }
  }, []);

  const connectCodex = useCallback(async () => {
    setCodex((c) => ({ ...c, state: "starting", error: undefined }));
    try {
      await api.codexStart();
      retries.current = 0;
      await refreshAccount();
    } catch (e) {
      setCodex({ state: "error", error: errorText(e) });
    }
  }, [refreshAccount]);

  // Look for a newer release once after start (silent when offline or the repository is private).
  useEffect(() => {
    const t = setTimeout(() => { void api.checkUpdate().then((u) => { if (u?.newer) setNewVersion(u.latest); }).catch(() => undefined); }, 8000);
    return () => clearTimeout(t);
  }, []);

  useEffect(() => {
    void refreshInfo();
    void connectCodex();
    const subs = [
      listen("codex-disconnected", () => {
        setCodex({ state: "error", error: "Codex 연결이 끊어졌습니다." });
        // Reconnect automatically a few times (e.g. after a Codex update).
        if (retries.current < 3) {
          retries.current += 1;
          setTimeout(() => void connectCodex(), 2000 * retries.current);
        }
      }),
      listen<{ method: string; params?: any }>("codex-event", (e) => {
        const { method: m, params } = e.payload;
        if (m === "account/login/completed") {
          const mine = !params?.loginId || params.loginId === loginRef.current?.loginId;
          if (mine) {
            setLogin(null);
            if (params?.success === false) setLoginError(params?.error || "로그인이 완료되지 않았습니다. 다시 시도해 주세요.");
            else setLoginError("");
          }
          void refreshAccount();
        }
        if (m === "account/updated") void refreshAccount();
      }),
      listen<{ count?: number }>("pending-changed", (e) => {
        if (typeof e.payload?.count === "number") setPendingCount(e.payload.count);
        else void refreshInfo();
      }),
    ];
    return () => { subs.forEach((p) => void p.then((f) => f())); };
  }, [refreshInfo, connectCodex, refreshAccount]);

  const startLogin = useCallback(async (mode: "browser" | "device") => {
    setLoginError("");
    try {
      if (codex.state !== "ready") await connectCodex();
      const r = await api.codex<any>("account/login/start", { type: mode === "browser" ? "chatgpt" : "chatgptDeviceCode" });
      const next: LoginState = { mode, loginId: r.loginId, authUrl: r.authUrl, userCode: r.userCode, verificationUrl: r.verificationUrl };
      setLogin(next);
      const url = mode === "browser" ? r.authUrl : r.verificationUrl;
      if (url) {
        // If the browser cannot be opened, the login panel shows the link to copy instead.
        try { await openExternal(url); } catch { setLogin({ ...next, openFailed: true }); }
      }
    } catch (e) {
      setLoginError(errorText(e));
    }
  }, [codex.state, connectCodex]);

  const cancelLogin = useCallback(async () => {
    const id = loginRef.current?.loginId;
    setLogin(null);
    if (id) { try { await api.codex("account/login/cancel", { loginId: id }); } catch { /* already finished */ } }
  }, []);

  const addAttachment = useCallback((a: Attachment) => {
    setAttachments((old) => (old.some((x) => x.key === a.key) ? old : [...old, a].slice(-20)));
    if (a.type === "file") setRecentFiles((old) => [a.file, ...old.filter((f) => f.id !== a.file.id)].slice(0, 20));
  }, []);
  const removeAttachment = useCallback((key: string) => setAttachments((old) => old.filter((x) => x.key !== key)), []);
  const clearAttachments = useCallback(() => setAttachments([]), []);

  const askAI = useCallback((text: string, opts?: { send?: boolean; refs?: CanvasRef[] }) => {
    for (const ref of opts?.refs ?? []) addAttachment({ type: "ref", key: `${ref.kind}:${ref.courseId}:${ref.id}`, ref });
    setPrefill((p) => ({ text, n: (p?.n ?? 0) + 1, send: opts?.send }));
    setTab("chat");
  }, [addAttachment]);

  const openMenu = useCallback((e: { clientX: number; clientY: number; preventDefault: () => void }, ref: CanvasRef) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, ref });
  }, []);

  const ctx: Ctx = {
    info, login, loginError, startLogin, cancelLogin, refreshInfo, codex, connectCodex, refreshAccount, go: setTab, askAI, pendingCount,
    attachments, recentFiles, addAttachment, removeAttachment, clearAttachments, openMenu, openSubmit: setSubmitRef,
  };
  const canvasReady = !!(info?.hasToken && info.settings.canvasBaseUrl);

  const nav: Array<{ id: Tab; label: string; icon: string }> = [
    { id: "chat", label: "AI 대화", icon: "💬" },
    { id: "canvas", label: "Canvas", icon: "📚" },
    { id: "messages", label: "메시지", icon: "✉️" },
    { id: "approvals", label: "승인 대기", icon: "✋" },
    { id: "automation", label: "자동화", icon: "⏰" },
    { id: "settings", label: "설정", icon: "⚙️" },
  ];

  return (
    <AppContext.Provider value={ctx}>
      <div className="app">
        <aside className="sidebar">
          <div className="brand">
            <img src="/logo.png" alt="" />
            <div><strong>Canvas Assistant</strong><small>v{info?.version ?? "…"}</small></div>
          </div>
          <nav aria-label="주 메뉴">
            {nav.map((n) => (
              <button key={n.id} className={"nav-item" + (tab === n.id ? " active" : "")} onClick={() => setTab(n.id)} aria-current={tab === n.id ? "page" : undefined}>
                <span className="nav-icon" aria-hidden>{n.icon}</span>
                <span>{n.label}</span>
                {n.id === "approvals" && pendingCount > 0 && <span className="count">{pendingCount}</span>}
              </button>
            ))}
          </nav>
          <div id="sidebar-slot" className="sidebar-slot" />
          <div className="status">
            <button className="status-row" onClick={() => setTab("settings")} title="설정 열기">
              <span className={"dot " + (codex.state === "ready" ? (codex.account ? "ok" : "warn") : codex.state === "starting" ? "idle" : "bad")} />
              Codex {codex.state === "ready" ? (codex.account ? "연결됨" : "로그인 필요") : codex.state === "starting" ? "연결 중…" : "연결 안 됨"}
            </button>
            <button className="status-row" onClick={() => setTab(canvasReady ? "canvas" : "settings")} title="Canvas">
              <span className={"dot " + (canvasReady ? "ok" : "warn")} />
              Canvas {canvasReady ? "연결됨" : "설정 필요"}
            </button>
          </div>
        </aside>
        <main className="main">
          {newVersion && tab !== "settings" && (
            <Banner tone="info" onClose={() => setNewVersion(null)}>
              새 버전 v{newVersion}이(가) 나왔습니다. <button className="link" onClick={() => setTab("settings")}>설정에서 업데이트</button>
            </Banner>
          )}
          {tab === "chat" && <Chat prefill={prefill} />}
          {tab === "canvas" && <CanvasView />}
          {tab === "messages" && <Messages />}
          {tab === "approvals" && <Approvals />}
          {tab === "automation" && <Automation />}
          {tab === "settings" && <SettingsView />}
        </main>
      </div>
      <ContextMenu state={menu} onClose={() => setMenu(null)} />
      {submitRef && <SubmitDialog target={submitRef} onClose={() => setSubmitRef(null)} />}
    </AppContext.Provider>
  );
}
