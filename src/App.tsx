import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, AppInfo, errorText } from "./api";
import Chat from "./views/Chat";
import CanvasView from "./views/Canvas";
import Approvals from "./views/Approvals";
import Automation from "./views/Automation";
import SettingsView from "./views/Settings";

export type Tab = "chat" | "canvas" | "approvals" | "automation" | "settings";

export type CodexStatus = {
  state: "starting" | "ready" | "error";
  error?: string;
  account?: { type: string; email?: string | null; planType?: string } | null;
};

type Ctx = {
  info: AppInfo | null;
  refreshInfo: () => Promise<void>;
  codex: CodexStatus;
  connectCodex: () => Promise<void>;
  refreshAccount: () => Promise<void>;
  go: (tab: Tab) => void;
  askAI: (prompt: string) => void;
  pendingCount: number;
};

const AppContext = createContext<Ctx | null>(null);
export const useApp = () => useContext(AppContext)!;

export default function App() {
  const [tab, setTab] = useState<Tab>("chat");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [codex, setCodex] = useState<CodexStatus>({ state: "starting" });
  const [pendingCount, setPendingCount] = useState(0);
  const [prefill, setPrefill] = useState<{ text: string; n: number } | null>(null);
  const retries = useRef(0);

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
      listen<{ method: string }>("codex-event", (e) => {
        const m = e.payload.method;
        if (m === "account/updated" || m === "account/login/completed") void refreshAccount();
      }),
      listen<{ count?: number }>("pending-changed", (e) => {
        if (typeof e.payload?.count === "number") setPendingCount(e.payload.count);
        else void refreshInfo();
      }),
    ];
    return () => { subs.forEach((p) => void p.then((f) => f())); };
  }, [refreshInfo, connectCodex, refreshAccount]);

  const askAI = useCallback((text: string) => {
    setPrefill((p) => ({ text, n: (p?.n ?? 0) + 1 }));
    setTab("chat");
  }, []);

  const ctx: Ctx = { info, refreshInfo, codex, connectCodex, refreshAccount, go: setTab, askAI, pendingCount };
  const canvasReady = !!(info?.hasToken && info.settings.canvasBaseUrl);

  const nav: Array<{ id: Tab; label: string; icon: string }> = [
    { id: "chat", label: "AI 대화", icon: "💬" },
    { id: "canvas", label: "Canvas", icon: "📚" },
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
          {tab === "chat" && <Chat prefill={prefill} />}
          {tab === "canvas" && <CanvasView />}
          {tab === "approvals" && <Approvals />}
          {tab === "automation" && <Automation />}
          {tab === "settings" && <SettingsView />}
        </main>
      </div>
    </AppContext.Provider>
  );
}
