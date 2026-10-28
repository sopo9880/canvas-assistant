import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorText, Pending } from "../api";
import { Badge, Banner, Empty, Spinner } from "../components";
import { formatDate, formatUnix } from "../util";

const kindLabel: Record<string, string> = {
  submit_text: "과제 텍스트 제출",
  submit_url: "과제 URL 제출",
  discussion_reply: "토론 댓글",
  planner_note: "할 일 추가",
  calendar_event: "캘린더 일정",
};
const statusLabel: Record<Pending["status"], [string, "neutral" | "ok" | "danger" | "warn" | "past"]> = {
  pending: ["대기", "warn"], running: ["실행 중", "neutral"], done: ["완료", "ok"], failed: ["실패", "danger"], rejected: ["거절", "past"],
};

function PayloadView({ p }: { p: Record<string, any> }) {
  const body = p.body ?? p.message ?? p.details ?? p.description;
  return (
    <dl className="payload">
      {p.course_id != null && <><dt>강의</dt><dd>{p._course_name ?? "이름 확인 불가"} <span className="muted small">#{p.course_id}</span></dd></>}
      {p.assignment_id != null && <><dt>과제</dt><dd>{p._assignment_name ?? "이름 확인 불가"} <span className="muted small">#{p.assignment_id}</span>
        {p._due_at && <div className="muted small">마감 {formatDate(p._due_at)}</div>}</dd></>}
      {p.topic_id != null && <><dt>토론</dt><dd>{p._topic_title ?? "제목 확인 불가"} <span className="muted small">#{p.topic_id}</span></dd></>}
      {p.title && <><dt>제목</dt><dd>{p.title}</dd></>}
      {(p.todo_date || p.start_at) && <><dt>날짜</dt><dd>{formatDate(p.todo_date ?? p.start_at) || (p.todo_date ?? p.start_at)}{p.end_at ? ` ~ ${formatDate(p.end_at) || p.end_at}` : ""}</dd></>}
      {p.url && <><dt>URL</dt><dd className="break">{p.url}</dd></>}
      {body && <><dt>내용</dt><dd><pre className="pre box">{String(body)}</pre></dd></>}
    </dl>
  );
}

export default function Approvals() {
  const [items, setItems] = useState<Pending[] | null>(null);
  const [history, setHistory] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [working, setWorking] = useState<number | null>(null);
  const [audit, setAudit] = useState<Array<{ at: number; action: string; detail: string; ok: boolean }> | null>(null);

  const load = useCallback(async () => {
    try { setItems(await api.pending(history)); } catch (e) { setError(errorText(e)); }
  }, [history]);

  useEffect(() => {
    void load();
    const un = listen("pending-changed", () => void load());
    return () => { void un.then((f) => f()); };
  }, [load]);

  async function decide(p: Pending, approve: boolean) {
    if (approve) {
      const warn = p.kind.startsWith("submit")
        ? "과제를 실제로 제출합니다. Canvas 설정에 따라 다시 제출할 수 없거나 제출 횟수가 차감될 수 있습니다. 계속할까요?"
        : "이 작업을 Canvas에 실행할까요?";
      if (!window.confirm(`${p.summary}\n\n${warn}`)) return;
    }
    setWorking(p.id);
    setError("");
    try {
      const r = await api.decide(p.id, approve);
      setNotice(approve ? `실행 완료: ${p.summary}` : "요청을 거절했습니다.");
      void r;
    } catch (e) {
      setError(errorText(e));
    } finally {
      setWorking(null);
      void load();
    }
  }

  return (
    <div className="page">
      <header className="topbar">
        <h1>승인 대기</h1>
        <div className="row">
          <label className="inline"><input type="checkbox" checked={history} onChange={(e) => setHistory(e.target.checked)} /> 처리한 요청도 보기</label>
          <button onClick={async () => { try { setAudit(await api.audit()); } catch (e) { setError(errorText(e)); } }}>활동 기록</button>
        </div>
      </header>
      <div className="content narrow">
        <p className="muted">AI는 Canvas를 직접 바꾸지 않습니다. 과제 제출·댓글·일정 추가 같은 요청은 여기 쌓이고, 승인해야만 실행됩니다.</p>
        {error && <Banner onClose={() => setError("")}>{error}</Banner>}
        {notice && <Banner tone="ok" onClose={() => setNotice("")}>{notice}</Banner>}
        {items === null ? <Spinner label="불러오는 중…" /> : items.length === 0 ? (
          <Empty title="승인을 기다리는 요청이 없습니다" icon="✅" />
        ) : (
          items.map((p) => (
            <article key={p.id} className="card approval">
              <div className="row space">
                <div className="row"><Badge tone="accent">{kindLabel[p.kind] ?? p.kind}</Badge><Badge tone={statusLabel[p.status][1]}>{statusLabel[p.status][0]}</Badge></div>
                <span className="muted small">#{p.id} · {formatUnix(p.createdAt)}</span>
              </div>
              <h3>{p.summary}</h3>
              {p.reason && <p className="muted">AI가 밝힌 이유: {p.reason}</p>}
              <PayloadView p={p.payload} />
              {p.result && p.status !== "pending" && <p className={"small " + (p.status === "failed" ? "error-text" : "muted")}>결과: {p.result}</p>}
              {p.status === "pending" && (
                <div className="row end">
                  <button disabled={working === p.id} onClick={() => void decide(p, false)}>거절</button>
                  <button className="primary" disabled={working === p.id} onClick={() => void decide(p, true)}>{working === p.id ? "실행 중…" : "승인하고 실행"}</button>
                </div>
              )}
            </article>
          ))
        )}
      </div>
      {audit && (
        <div className="modal" role="dialog" aria-modal onClick={() => setAudit(null)}>
          <div className="modal-body wide" onClick={(e) => e.stopPropagation()}>
            <h2>활동 기록</h2>
            {audit.length === 0 ? <Empty title="기록이 없습니다" /> : (
              <table className="table">
                <thead><tr><th>시각</th><th>작업</th><th>내용</th><th>결과</th></tr></thead>
                <tbody>{audit.map((a, i) => <tr key={i}><td className="small">{formatUnix(a.at)}</td><td>{a.action}</td><td>{a.detail}</td><td>{a.ok ? "성공" : "실패"}</td></tr>)}</tbody>
              </table>
            )}
            <div className="row end"><button className="primary" onClick={() => setAudit(null)}>닫기</button></div>
          </div>
        </div>
      )}
    </div>
  );
}
