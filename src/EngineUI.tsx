import { useEffect, useState } from "react";
import { api, EngineStatus, errorText } from "./api";
import { useApp } from "./App";
import { Banner, Spinner, Toggle } from "./components";
import { bytes } from "./util";

const STAGES: Record<string, string> = {
  resolve: "버전 확인 중…",
  download: "AI 엔진 내려받는 중",
  unpack: "압축 푸는 중…",
  verify: "호환성 확인 중…",
};

/** Progress of a running engine install/update, or its error with a retry button. */
export function EngineProgress() {
  const { engine, installEngine } = useApp();
  if (engine.phase === "error") {
    return (
      <div className="login-box">
        <Banner>{engine.error}</Banner>
        <div className="row"><button className="primary" onClick={() => void installEngine(engine.target)}>다시 시도</button></div>
      </div>
    );
  }
  if (engine.phase !== "installing") return null;
  const pct = engine.total > 0 ? Math.min(100, Math.round((engine.received / engine.total) * 100)) : 0;
  return (
    <div className="login-box">
      <Spinner label={`${STAGES[engine.stage] ?? "설치 중…"}${engine.stage === "download" && engine.total ? ` ${bytes(engine.received)} / ${bytes(engine.total)} (${pct}%)` : ""}`} />
      {engine.stage === "download" && engine.total > 0 && <progress value={engine.received} max={engine.total} />}
      <p className="muted small">처음 한 번만 받습니다. 로그인 정보와 대화는 그대로 유지됩니다.</p>
    </div>
  );
}

/** Settings card: which Codex is installed, and the update / rollback buttons. */
export function EngineCard() {
  const { engine, installEngine, checkEngine, connectCodex, info, refreshInfo } = useApp();
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [note, setNote] = useState<{ tone: "ok" | "error" | "info"; text: string } | null>(null);
  const busy = engine.phase === "installing";

  const reload = async () => { try { setStatus(await api.engineStatus()); } catch { /* shown elsewhere */ } };
  useEffect(() => { void reload(); }, [engine.phase]);

  const check = async () => {
    setChecking(true);
    setNote(null);
    const r = await checkEngine();
    setChecking(false);
    if (!r) setNote({ tone: "error", text: "최신 버전을 확인하지 못했습니다. 인터넷 연결을 확인하세요." });
    else if (r.newer && r.installed) setNote({ tone: "info", text: `새 버전 ${r.latest}이(가) 있습니다.` });
    else if (r.newer) setNote({ tone: "info", text: `최신 버전은 ${r.latest}입니다.` });
    else setNote({ tone: "ok", text: `최신 버전입니다 (${r.latest}).` });
  };
  const update = async () => {
    if (!window.confirm("AI 엔진을 업데이트할까요? 새 버전을 내려받아 검증한 뒤 바꿉니다. 문제가 있으면 이전 버전으로 되돌릴 수 있고, 로그인과 대화는 그대로입니다.")) return;
    setNote(null);
    const ok = await installEngine("latest");
    if (ok) setNote({ tone: "ok", text: "업데이트했습니다." });
    void reload();
  };
  const rollback = async () => {
    if (!window.confirm(`이전 버전(${status?.previous})으로 되돌릴까요?`)) return;
    setNote(null);
    try { await api.engineRollback(); await connectCodex(); setNote({ tone: "ok", text: "이전 버전으로 되돌렸습니다." }); }
    catch (e) { setNote({ tone: "error", text: errorText(e) }); }
    void reload();
  };

  if (!status) return null;
  const latest = engine.latest;
  return (
    <div className="engine-card">
      <h3>AI 엔진 (Codex)</h3>
      <div className="kv">
        <span>설치된 버전</span>
        <span>{status.installed ? `v${status.installed}` : status.legacy ? "설치 프로그램에 포함된 버전 (구버전 설치)" : "설치되지 않음"}</span>
        {status.previous && (<><span>이전 버전</span><span>v{status.previous} (되돌리기 가능)</span></>)}
        <span>업데이트</span>
        <span>{latest ? `새 버전 v${latest} 사용 가능` : info?.settings.engineAutoUpdate ? "자동 업데이트 켜짐" : "자동으로 바꾸지 않습니다. 버튼을 눌렀을 때만 업데이트합니다."}</span>
      </div>
      <Toggle label="새 버전이 나오면 자동으로 업데이트" hint="켜면 새 안정 버전을 받아 검증한 뒤 알아서 바꿉니다. AI가 답하는 중에는 기다렸다가 합니다. 끄면 버튼을 누를 때만 업데이트합니다."
        checked={!!info?.settings.engineAutoUpdate}
        onChange={(v) => void api.saveSettings({ engineAutoUpdate: v }).then(async () => { await refreshInfo(); if (v) void checkEngine(); })} />
      <EngineProgress />
      {engine.autoError && <Banner>{engine.autoError}</Banner>}
      {note && <Banner tone={note.tone}>{note.text}</Banner>}
      <div className="row">
        {!status.installed && status.supported && <button className="primary" disabled={busy} onClick={() => void installEngine("pinned")}>{status.legacy ? "앱이 관리하는 엔진으로 전환" : "AI 엔진 설치"}</button>}
        {status.installed && latest && <button className="primary" disabled={busy} onClick={() => void update()}>v{latest}로 업데이트</button>}
        <button disabled={busy || checking} onClick={() => void check()}>{checking ? "확인 중…" : "엔진 업데이트 확인"}</button>
        {status.previous && <button disabled={busy} onClick={() => void rollback()}>이전 버전으로 되돌리기</button>}
      </div>
    </div>
  );
}
