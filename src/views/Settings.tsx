import { useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api, errorText } from "../api";
import { useApp } from "../App";
import { Banner, open, Spinner, Toggle } from "../components";

export default function SettingsView() {
  const { info, refreshInfo, codex, connectCodex, refreshAccount } = useApp();
  const s = info?.settings;
  const [baseUrl, setBaseUrl] = useState(s?.canvasBaseUrl ?? "");
  const [token, setToken] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [canvasMsg, setCanvasMsg] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const [codexPath, setCodexPath] = useState(s?.codexPath ?? "");
  const [located, setLocated] = useState<{ found: boolean; path?: string; error?: string } | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [msg, setMsg] = useState<{ tone: "ok" | "error" | "info"; text: string } | null>(null);
  const [update, setUpdate] = useState<any>(null);

  useEffect(() => { setBaseUrl(s?.canvasBaseUrl ?? ""); setCodexPath(s?.codexPath ?? ""); }, [s?.canvasBaseUrl, s?.codexPath]);
  useEffect(() => {
    void isEnabled().then(setAutostart).catch(() => setAutostart(false));
    void api.codexLocate().then(setLocated).catch(() => {});
  }, []);

  async function connect() {
    setConnecting(true);
    setCanvasMsg(null);
    try {
      const r = await api.canvasConnect(baseUrl, token);
      setToken("");
      setCanvasMsg({ tone: "ok", text: `${r.profile?.name ?? "사용자"}님으로 연결했습니다 (${r.baseUrl}).` });
      await refreshInfo();
    } catch (e) {
      setCanvasMsg({ tone: "error", text: errorText(e) });
    } finally { setConnecting(false); }
  }

  async function disconnect() {
    if (!window.confirm("저장된 Canvas 토큰을 삭제하고 연결을 해제할까요?")) return;
    try { await api.canvasDisconnect(); setCanvasMsg({ tone: "ok", text: "연결을 해제하고 토큰을 삭제했습니다." }); await refreshInfo(); }
    catch (e) { setCanvasMsg({ tone: "error", text: errorText(e) }); }
  }

  async function setPref(patch: Record<string, unknown>) {
    try { await api.saveSettings(patch); await refreshInfo(); } catch (e) { setMsg({ tone: "error", text: errorText(e) }); }
  }

  async function saveCodexPath() {
    try {
      await api.saveSettings({ codexPath: codexPath.trim() || null });
      setLocated(await api.codexLocate());
      await refreshInfo();
      await connectCodex();
      setMsg({ tone: "ok", text: "Codex 경로를 저장하고 다시 연결했습니다." });
    } catch (e) { setMsg({ tone: "error", text: errorText(e) }); }
  }

  async function toggleAutostart(v: boolean) {
    try { if (v) await enable(); else await disable(); setAutostart(await isEnabled()); }
    catch (e) { setMsg({ tone: "error", text: `자동 실행 설정 실패: ${errorText(e)}` }); }
  }

  async function logout() {
    if (!window.confirm("Codex(ChatGPT) 계정에서 로그아웃할까요?")) return;
    try { await api.codex("account/logout"); await refreshAccount(); } catch (e) { setMsg({ tone: "error", text: errorText(e) }); }
  }

  const tokenPage = (() => {
    try { return baseUrl ? `${new URL(baseUrl.includes("://") ? baseUrl : `https://${baseUrl}`).origin}/profile/settings` : null; } catch { return null; }
  })();
  const connected = !!(info?.hasToken && s?.canvasBaseUrl);

  return (
    <div className="page">
      <header className="topbar"><h1>설정</h1></header>
      <div className="content narrow">
        {msg && <Banner tone={msg.tone} onClose={() => setMsg(null)}>{msg.text}</Banner>}

        <section className="card">
          <h2>📚 Canvas 연결</h2>
          <p className="muted">학교 Canvas 주소와 액세스 토큰을 입력하세요. 토큰은 Windows 자격 증명 관리자에만 저장되고 화면이나 파일에 남지 않습니다.</p>
          <form className="form-grid" onSubmit={(e) => { e.preventDefault(); void connect(); }}>
            <label className="span-2">Canvas 주소
              <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="예: canvas.학교.ac.kr" autoComplete="url" spellCheck={false} />
            </label>
            <label className="span-2"><span>액세스 토큰{connected && <span className="muted small"> · 저장됨 (바꿀 때만 입력)</span>}</span>
              <input type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder={connected ? "••••••••" : "Canvas에서 발급한 토큰"} autoComplete="off" spellCheck={false} />
            </label>
            <div className="span-2 row space">
              <button type="button" className="link" disabled={!tokenPage} onClick={() => open(tokenPage)}>
                토큰 발급 페이지 열기 (계정 → 설정 → 새 액세스 토큰) →
              </button>
              <div className="row">
                {connected && <button type="button" onClick={() => void disconnect()}>연결 해제</button>}
                <button type="submit" className="primary" disabled={connecting || !baseUrl.trim() || (!connected && !token.trim())}>
                  {connecting ? "확인 중…" : connected ? "다시 확인 / 저장" : "연결"}
                </button>
              </div>
            </div>
          </form>
          {canvasMsg && <Banner tone={canvasMsg.tone} onClose={() => setCanvasMsg(null)}>{canvasMsg.text}</Banner>}
          <Toggle label="AI 대화에서 Canvas 도구 사용" hint="끄면 AI가 Canvas 데이터를 조회하지 않습니다. 새 대화부터 적용됩니다."
            checked={s?.canvasToolsInChat ?? true} onChange={(v) => void setPref({ canvasToolsInChat: v })} />
        </section>

        <section className="card">
          <h2>🤖 Codex (ChatGPT)</h2>
          <div className="kv">
            <span>상태</span>
            <span>{codex.state === "starting" ? <Spinner label="연결 중" /> : codex.state === "error" ? <span className="error-text">{codex.error}</span> : codex.account ? `로그인됨 — ${codex.account.email ?? codex.account.type}${codex.account.planType ? ` (${codex.account.planType})` : ""}` : "로그인 필요 (AI 대화 화면에서 로그인)"}</span>
            <span>Codex CLI</span>
            <span className="break">{located ? (located.found ? located.path : <span className="error-text">{located.error}</span>) : "확인 중…"}</span>
          </div>
          <div className="row">
            <button onClick={() => void connectCodex()}>다시 연결</button>
            {codex.account && <button onClick={() => void logout()}>로그아웃</button>}
          </div>
          <form className="form-grid" onSubmit={(e) => { e.preventDefault(); void saveCodexPath(); }}>
            <label className="span-2">codex.exe 경로 (자동으로 찾지 못할 때만)
              <input value={codexPath} onChange={(e) => setCodexPath(e.target.value)} placeholder="C:\...\codex.exe" spellCheck={false} />
            </label>
            <div className="span-2 row end"><button type="submit">경로 저장</button></div>
          </form>
          <p className="muted small">Codex CLI가 없다면 Node.js를 설치한 뒤 PowerShell에서 <code>npm install -g @openai/codex</code>를 실행하세요. AI는 읽기 전용으로만 실행되며 PC의 파일이나 명령에 접근하지 않습니다.</p>
        </section>

        <section className="card">
          <h2>🪟 Windows</h2>
          <Toggle label="Windows 시작 시 자동 실행" hint="트레이에 조용히 시작해 알림과 자동화를 계속 실행합니다."
            checked={!!autostart} disabled={autostart === null} onChange={(v) => void toggleAutostart(v)} />
          <Toggle label="창을 닫으면 트레이로 숨기기" hint="끄면 창을 닫을 때 앱이 완전히 종료됩니다."
            checked={s?.closeToTray ?? true} onChange={(v) => void setPref({ closeToTray: v })} />
          <Toggle label="Windows 알림" hint="마감·공지·성적·승인 요청 알림"
            checked={s?.notifications ?? true} onChange={(v) => void setPref({ notifications: v })} />
        </section>

        <section className="card">
          <h2>ℹ️ 정보</h2>
          <div className="kv">
            <span>버전</span><span>v{info?.version}</span>
            <span>데이터 폴더</span><span className="break">{info?.dataDir}</span>
          </div>
          <div className="row">
            <button onClick={async () => {
              setUpdate({ loading: true });
              try { setUpdate(await api.checkUpdate()); } catch (e) { setUpdate({ error: errorText(e) }); }
            }}>업데이트 확인</button>
            <button onClick={() => open("https://github.com/sopo9880/canvas-assistant/releases")}>릴리스 페이지</button>
          </div>
          {update && (update.loading ? <Spinner label="확인 중…" /> : update.error ? <Banner>{update.error}</Banner> : !update.available ? (
            <Banner tone="info">자동 확인을 할 수 없습니다 (비공개 저장소). 릴리스 페이지에서 직접 확인하세요.</Banner>
          ) : update.newer ? (
            <Banner tone="ok">새 버전 v{update.latest}이(가) 있습니다. <button className="link" onClick={() => open(update.url)}>내려받기 →</button></Banner>
          ) : <Banner tone="ok">최신 버전입니다 (v{update.current}).</Banner>)}
        </section>
      </div>
    </div>
  );
}
