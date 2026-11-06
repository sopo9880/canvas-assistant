import { useState } from "react";
import { api, errorText } from "../api";
import { useApp } from "../App";
import { Banner, Empty, Spinner } from "../components";
import { canvasAction, useLoad } from "../shared";
import { formatDate } from "../util";

export default function Messages() {
  const { info, go, openMenu } = useApp();
  const [scope, setScope] = useState("inbox");
  const list = useLoad(() => api.canvas<any[]>("canvas_conversations", { scope }), [scope]);
  const [openId, setOpenId] = useState<string | null>(null);
  const [compose, setCompose] = useState(false);
  const configured = !!(info?.hasToken && info.settings.canvasBaseUrl);

  if (!configured) {
    return (
      <div className="page"><header className="topbar"><h1>메시지</h1></header>
        <div className="content"><Empty title="Canvas를 먼저 연결하세요" icon="✉️"><button className="primary" onClick={() => go("settings")}>설정으로</button></Empty></div>
      </div>
    );
  }

  return (
    <div className="page">
      <header className="topbar">
        <h1>메시지</h1>
        <div className="row">
          <select value={scope} onChange={(e) => { setScope(e.target.value); setOpenId(null); }} aria-label="메시지함">
            <option value="inbox">받은 메시지</option><option value="unread">안 읽음</option><option value="starred">별표</option><option value="sent">보낸 메시지</option><option value="archived">보관</option>
          </select>
          <button className="primary" onClick={() => setCompose(true)}>✏️ 새 메시지</button>
          <button onClick={() => void list.reload()} title="새로고침">↻</button>
        </div>
      </header>
      <div className="content split">
        <section className="card list-pane">
          {list.loading ? <Spinner /> : list.error ? <Banner>{list.error}</Banner> : (list.data ?? []).length === 0 ? <Empty title="메시지가 없습니다" icon="📭" /> : (
            <ul className="list">
              {list.data!.map((c) => (
                <li key={c.id} className={"list-item clickable" + (openId === String(c.id) ? " selected" : "")} onClick={() => setOpenId(String(c.id))}
                  onContextMenu={(e) => openMenu(e, { kind: "message", label: c.subject || "(제목 없음)", id: String(c.id) })}>
                  <div className="grow">
                    <div className="title">{c.workflow_state === "unread" && <span className="dot ok inline" aria-label="안 읽음" />} {c.subject || "(제목 없음)"}</div>
                    <div className="muted small">{(c.participants ?? []).slice(0, 3).join(", ")} · {c.context_name}</div>
                    <div className="muted small ellipsis">{c.last_message}</div>
                  </div>
                  <span className="muted small">{formatDate(c.last_message_at)}</span>
                </li>
              ))}
            </ul>
          )}
        </section>
        <section className="card detail-pane">
          {openId ? <Conversation id={openId} onChanged={() => void list.reload()} /> : <Empty title="메시지를 선택하세요" icon="✉️">우클릭하면 AI에게 요약이나 답장 초안을 부탁할 수 있습니다.</Empty>}
        </section>
      </div>
      {compose && <Compose onClose={() => setCompose(false)} onSent={() => { setCompose(false); void list.reload(); }} />}
    </div>
  );
}

function Conversation({ id, onChanged }: { id: string; onChanged: () => void }) {
  const c = useLoad(() => api.canvas<any>("canvas_conversation", { conversation_id: id }), [id]);
  const [reply, setReply] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  if (c.loading) return <Spinner label="불러오는 중…" />;
  if (c.error) return <Banner>{c.error}</Banner>;
  const d = c.data!;
  return (
    <div className="conversation-view">
      <h2>{d.subject || "(제목 없음)"}</h2>
      <p className="muted small">{(d.participants ?? []).map((p: any) => p.name).join(", ")} · {d.context_name}</p>
      <div className="messages">
        {[...(d.messages ?? [])].reverse().map((m: any) => (
          <div key={m.id} className="message">
            <div className="row space"><strong>{m.author}</strong><span className="muted small">{formatDate(m.created_at)}</span></div>
            <p className="pre">{m.body}</p>
            {m.attachments?.length > 0 && <p className="muted small">📎 {m.attachments.map((a: any) => a.display_name).join(", ")}</p>}
          </div>
        ))}
      </div>
      {msg && <Banner tone={msg.tone} onClose={() => setMsg(null)}>{msg.text}</Banner>}
      <label>답장<textarea rows={3} value={reply} onChange={(e) => setReply(e.target.value)} placeholder="답장을 입력하세요" /></label>
      <div className="row end">
        <button className="primary" disabled={!reply.trim() || busy} onClick={async () => {
          if (!window.confirm("이 답장을 보낼까요? 보낸 메시지는 취소할 수 없습니다.")) return;
          setBusy(true);
          try { await canvasAction("conversation_reply", { conversation_id: id, body: reply }); setReply(""); setMsg({ tone: "ok", text: "보냈습니다." }); await c.reload(); onChanged(); }
          catch (e) { setMsg({ tone: "error", text: errorText(e) }); } finally { setBusy(false); }
        }}>{busy ? "보내는 중…" : "답장 보내기"}</button>
      </div>
    </div>
  );
}

function Compose({ onClose, onSent }: { onClose: () => void; onSent: () => void }) {
  const courses = useLoad(() => api.canvas<any[]>("canvas_list_courses"), []);
  const [course, setCourse] = useState("");
  const people = useLoad(() => (course ? api.canvas<any[]>("canvas_course_people", { course_id: course }) : Promise.resolve([])), [course]);
  const [filter, setFilter] = useState("");
  const [to, setTo] = useState<Array<{ id: string; name: string }>>([]);
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const shown = (people.data ?? []).filter((p) => !filter || String(p.name).includes(filter)).slice(0, 50);
  const role = (p: any) => (p.roles ?? []).includes("TeacherEnrollment") ? "교수" : (p.roles ?? []).includes("TaEnrollment") ? "조교" : "";
  return (
    <div className="modal" role="dialog" aria-modal aria-label="새 메시지" onClick={busy ? undefined : onClose}>
      <div className="modal-body wide" onClick={(e) => e.stopPropagation()}>
        <h2>✏️ 새 메시지</h2>
        {error && <Banner onClose={() => setError("")}>{error}</Banner>}
        <div className="form-grid">
          <label>강의
            <select value={course} onChange={(e) => { setCourse(e.target.value); setTo([]); }}>
              <option value="">강의 선택</option>
              {(courses.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
          </label>
          <label>받는 사람 검색<input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="이름" disabled={!course} /></label>
        </div>
        {course && (people.loading ? <Spinner /> : (
          <div className="people-pick">
            {shown.map((p) => {
              const on = to.some((t) => t.id === String(p.id));
              return <button key={p.id} className={on ? "chip on" : "chip"} onClick={() => setTo((old) => on ? old.filter((t) => t.id !== String(p.id)) : [...old, { id: String(p.id), name: p.name }].slice(0, 20))}>{p.name}{role(p) && ` (${role(p)})`}</button>;
            })}
          </div>
        ))}
        <p className="small">받는 사람: {to.length ? to.map((t) => t.name).join(", ") : <span className="muted">없음</span>}</p>
        <label>제목<input value={subject} onChange={(e) => setSubject(e.target.value)} maxLength={255} /></label>
        <label>내용<textarea rows={6} value={body} onChange={(e) => setBody(e.target.value)} /></label>
        <div className="row end">
          <button onClick={onClose} disabled={busy}>취소</button>
          <button className="primary" disabled={busy || !to.length || !body.trim()} onClick={async () => {
            if (!window.confirm(`${to.map((t) => t.name).join(", ")}에게 메시지를 보낼까요?`)) return;
            setBusy(true);
            try { await canvasAction("send_message", { recipients: to.map((t) => t.id), subject, body, course_id: course }); onSent(); }
            catch (e) { setError(errorText(e)); } finally { setBusy(false); }
          }}>{busy ? "보내는 중…" : "보내기"}</button>
        </div>
      </div>
    </div>
  );
}
