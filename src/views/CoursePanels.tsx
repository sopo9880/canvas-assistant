import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "../api";
import { useApp } from "../App";
import { Badge, Banner, Empty, Markdown, open, Spinner } from "../components";
import { canvasAction, CanvasRef, Due, useLoad } from "../shared";
import { assignmentStatus, bytes, formatDate, isSubmitted } from "../util";
import type { Course } from "./CourseView";

type P = { course: Course };
const ref = (c: Course, r: Omit<CanvasRef, "courseId" | "courseName">): CanvasRef => ({ ...r, courseId: c.id, courseName: c.name });

function Loading<T>({ r, empty, children }: { r: { loading: boolean; error: string; data: T | null }; empty?: string; children: (d: T) => React.ReactNode }) {
  if (r.loading) return <Spinner label="불러오는 중…" />;
  if (r.error) return <Banner>{r.error}</Banner>;
  const d = r.data as T;
  if (empty && Array.isArray(d) && d.length === 0) return <Empty title={empty} />;
  return <>{children(d)}</>;
}

function Modal({ onClose, children, wide = true }: { onClose: () => void; children: React.ReactNode; wide?: boolean }) {
  return (
    <div className="modal" role="dialog" aria-modal onClick={onClose}>
      <div className={"modal-body" + (wide ? " wide" : "")} onClick={(e) => e.stopPropagation()}>{children}</div>
    </div>
  );
}

// ---------------- 홈 ----------------
export function Home({ course }: P) {
  const { openMenu } = useApp();
  const home = useLoad(() => api.canvas<any>("canvas_course_home", { course_id: course.id }), [course.id]);
  const upcoming = useLoad(() => api.canvas<any[]>("canvas_list_assignments", { course_id: course.id, bucket: "upcoming" }), [course.id]);
  return (
    <>
      <section className="card">
        <h2>다가오는 과제</h2>
        <Loading r={upcoming} empty="다가오는 과제가 없습니다">{(list) => (
          <ul className="list">{list.map((a: any) => <AssignmentRow key={a.id} a={a} course={course} onContext={openMenu} />)}</ul>
        )}</Loading>
      </section>
      <Loading r={home}>{(h) => (
        <>
          {h.front_page?.body && (
            <section className="card"><h2>{h.front_page.title || "강의 홈"}</h2><p className="pre">{h.front_page.body}</p></section>
          )}
          <section className="card">
            <h2>{course.name}의 최근 활동</h2>
            {(h.recent_activity ?? []).length === 0 ? <Empty title="최근 활동이 없습니다" /> : (
              <ul className="list">
                {h.recent_activity.map((a: any, i: number) => (
                  <li key={i} className="list-item clickable" onClick={() => open(a.html_url)}
                    onContextMenu={(e) => openMenu(e, ref(course, { kind: a.type === "Announcement" ? "announcement" : a.type === "DiscussionTopic" ? "discussion" : "page", label: a.title, htmlUrl: a.html_url }))}>
                    <Badge>{({ Announcement: "공지", DiscussionTopic: "토론", Submission: "제출", Message: "알림", Conversation: "메시지" } as Record<string, string>)[a.type] ?? a.type}</Badge>
                    <div className="grow"><div className="title">{a.title}</div>{a.message && <div className="muted small ellipsis">{a.message}</div>}</div>
                    <span className="muted small">{formatDate(a.updated_at ?? a.created_at)}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </>
      )}</Loading>
    </>
  );
}

// ---------------- 수업 계획서 ----------------
export function Syllabus({ course }: P) {
  const { askAI } = useApp();
  const r = useLoad(() => api.canvas<any>("canvas_syllabus", { course_id: course.id }), [course.id]);
  const groups = useLoad(() => api.canvas<any[]>("canvas_assignment_groups", { course_id: course.id }), [course.id]);
  return (
    <>
      <section className="card">
        <div className="row space">
          <h2>📋 수업 계획서</h2>
          <button onClick={() => askAI("첨부한 강의의 수업 계획서를 요약해줘. 평가 비율, 시험 일정, 출석·과제 규정을 표로 정리해줘.", { refs: [{ kind: "course", label: course.name, courseId: course.id }] })}>✨ AI 요약</button>
        </div>
        <Loading r={r}>{(d) => d.syllabus ? <p className="pre">{d.syllabus}</p> : <Empty title="수업 계획서가 비어 있습니다" />}</Loading>
      </section>
      <section className="card">
        <h2>평가 항목과 비율</h2>
        <Loading r={groups} empty="평가 항목이 없습니다">{(list) => (
          <table className="table">
            <thead><tr><th>항목</th><th>비율</th><th>과제 수</th></tr></thead>
            <tbody>{list.map((g: any) => <tr key={g.id}><td>{g.name}</td><td>{g.group_weight ? `${g.group_weight}%` : "—"}</td><td>{g.assignments?.length ?? 0}</td></tr>)}</tbody>
          </table>
        )}</Loading>
      </section>
    </>
  );
}

// ---------------- 공지 / 게시판 ----------------
function TopicList({ course, announcements }: P & { announcements: boolean }) {
  const { openMenu } = useApp();
  const r = useLoad(() => announcements
    ? api.canvas<any[]>("canvas_list_announcements", { course_ids: [course.id], days_back: 365 })
    : api.canvas<any[]>("canvas_list_discussions", { course_id: course.id }), [course.id, announcements]);
  const [topic, setTopic] = useState<any>(null);
  const kind = announcements ? "announcement" : "discussion";
  return (
    <section className="card">
      <h2>{announcements ? "📢 공지" : "💬 게시판"}</h2>
      <Loading r={r} empty={announcements ? "공지가 없습니다" : "게시글이 없습니다"}>{(list) => (
        <ul className="list">
          {list.map((t: any) => (
            <li key={t.id} className="list-item clickable" onClick={() => setTopic(t)}
              onContextMenu={(e) => openMenu(e, ref(course, { kind, label: t.title, id: String(t.id), htmlUrl: t.html_url }))}>
              <div className="grow">
                <div className="title">{t.read_state === "unread" && <span className="dot ok inline" aria-label="안 읽음" />} {t.title}</div>
                <div className="muted small">{t.author ? `${t.author} · ` : ""}{formatDate(t.posted_at ?? t.last_reply_at)}{t.discussion_subentry_count != null ? ` · 댓글 ${t.discussion_subentry_count}` : ""}{t.unread_count ? ` · 안 읽음 ${t.unread_count}` : ""}</div>
              </div>
              <span className="chev" aria-hidden>›</span>
            </li>
          ))}
        </ul>
      )}</Loading>
      {topic && <Thread course={course} topicId={String(topic.id)} kind={kind} onClose={() => { setTopic(null); void r.reload(); }} />}
    </section>
  );
}
export const Announcements = ({ course }: P) => <TopicList course={course} announcements />;
export const Discussions = ({ course }: P) => <TopicList course={course} announcements={false} />;

function Thread({ course, topicId, kind, onClose }: P & { topicId: string; kind: "announcement" | "discussion"; onClose: () => void }) {
  const { askAI } = useApp();
  const r = useLoad(() => api.canvas<any>("canvas_discussion_thread", { course_id: course.id, topic_id: topicId }), [topicId]);
  const [replyTo, setReplyTo] = useState<{ id?: string; author?: string } | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const send = async () => {
    if (!text.trim() || !window.confirm("이 글을 Canvas에 올릴까요? 다른 학생들도 볼 수 있습니다.")) return;
    setBusy(true);
    try {
      if (replyTo?.id) await canvasAction("discussion_entry_reply", { course_id: course.id, topic_id: topicId, entry_id: replyTo.id, message: text });
      else await canvasAction("discussion_reply", { course_id: course.id, topic_id: topicId, message: text });
      setText(""); setReplyTo(null); setMsg({ tone: "ok", text: "올렸습니다." }); await r.reload();
    } catch (e) { setMsg({ tone: "error", text: errorText(e) }); } finally { setBusy(false); }
  };
  const renderEntries = (entries: any[], depth = 0): React.ReactNode => entries.map((e) => (
    <div key={e.id} className="entry" style={{ marginLeft: depth * 18 }}>
      <div className="row space"><strong>{e.author || "알 수 없음"}</strong><span className="muted small">{formatDate(e.created_at)}</span></div>
      <p className="pre">{e.deleted ? <span className="muted">(삭제된 글)</span> : e.message}</p>
      {!e.deleted && <button className="link small" onClick={() => setReplyTo({ id: String(e.id), author: e.author })}>답글</button>}
      {e.replies && renderEntries(e.replies, depth + 1)}
    </div>
  ));
  return (
    <Modal onClose={onClose}>
      <Loading r={r}>{(t) => (
        <>
          <div className="row space">
            <h2>{t.title}</h2>
            <div className="row">
              <button onClick={() => askAI(kind === "announcement" ? "첨부한 공지를 요약하고 내가 해야 할 일이 있으면 알려줘." : "첨부한 토론의 주제와 지금까지 나온 의견을 요약해줘.", { refs: [ref(course, { kind, label: t.title, id: topicId, htmlUrl: t.html_url })] })}>✨ AI</button>
              <button onClick={async () => { try { await canvasAction("discussion_mark_read", { course_id: course.id, topic_id: topicId }); setMsg({ tone: "ok", text: "모두 읽음으로 표시했습니다." }); } catch (e) { setMsg({ tone: "error", text: errorText(e) }); } }}>모두 읽음</button>
              <button onClick={() => open(t.html_url)}>Canvas</button>
            </div>
          </div>
          <p className="muted small">{t.author} · {formatDate(t.posted_at)}</p>
          {t.message && <p className="pre">{t.message}</p>}
          {msg && <Banner tone={msg.tone} onClose={() => setMsg(null)}>{msg.text}</Banner>}
          {t.entries_note && <Banner tone="info">{t.entries_note}</Banner>}
          {(t.entries ?? []).length > 0 && <div className="entries"><h3>댓글 {t.entries.length}</h3>{renderEntries(t.entries)}</div>}
          {!t.locked && kind === "discussion" && (
            <>
              <label>{replyTo?.id ? <>답글 → {replyTo.author} <button className="link small" onClick={() => setReplyTo(null)}>취소</button></> : "새 댓글"}
                <textarea rows={3} value={text} onChange={(e) => setText(e.target.value)} />
              </label>
              <div className="row end"><button className="primary" disabled={!text.trim() || busy} onClick={() => void send()}>{busy ? "올리는 중…" : "올리기"}</button></div>
            </>
          )}
          <div className="row end"><button onClick={onClose}>닫기</button></div>
        </>
      )}</Loading>
    </Modal>
  );
}

// ---------------- 강의자료실 ----------------
export function Files({ course }: P) {
  const { openMenu } = useApp();
  const folders = useLoad(() => api.canvas<any[]>("canvas_folders", { course_id: course.id }), [course.id]);
  const [folder, setFolder] = useState<any>(null);
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const files = useLoad(() => query
    ? api.canvas<any[]>("canvas_list_files", { course_id: course.id, search: query })
    : folder ? api.canvas<any[]>("canvas_folder_files", { folder_id: folder.id }) : Promise.resolve(null as any), [folder?.id, query]);
  const [msg, setMsg] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const [downloading, setDownloading] = useState<string | null>(null);
  const tree = useMemo(() => (folders.data ?? []).slice().sort((a, b) => String(a.full_name).localeCompare(String(b.full_name))), [folders.data]);
  const depth = (f: any) => Math.max(0, String(f.full_name).split("/").length - 1);
  return (
    <section className="card">
      <div className="row space">
        <h2>📁 강의자료실</h2>
        <form className="row" onSubmit={(e) => { e.preventDefault(); setQuery(search.trim()); }}>
          <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="파일 이름 검색 (2자 이상)" aria-label="파일 검색" />
          <button type="submit">검색</button>
          {query && <button type="button" className="ghost" onClick={() => { setQuery(""); setSearch(""); }}>✕</button>}
        </form>
      </div>
      {msg && <Banner tone={msg.tone} onClose={() => setMsg(null)}>{msg.text}</Banner>}
      <div className="files-layout">
        <div className="folder-tree">
          {folders.error && <p className="muted small">이 강의는 Canvas 파일 메뉴가 닫혀 있을 수 있습니다. 왼쪽 메뉴에 LearningX '강의자료실'이 있으면 그곳에서 자료를 확인하세요.</p>}
          <Loading r={folders} empty="폴더가 없습니다">{() => tree.map((f) => (
            <button key={f.id} className={"folder" + (folder?.id === f.id && !query ? " active" : "")} style={{ paddingLeft: 8 + depth(f) * 14 }} onClick={() => { setQuery(""); setFolder(f); }}>
              📂 {depth(f) === 0 ? "전체 자료" : f.name} <span className="muted small">{f.files_count || ""}</span>
            </button>
          ))}</Loading>
        </div>
        <div className="grow">
          {!folder && !query ? <Empty title="왼쪽에서 폴더를 선택하세요" icon="📂" /> : (
            <Loading r={files} empty="파일이 없거나 접근 권한이 없습니다">{(list) => (
              <ul className="list">
                {(list ?? []).map((f: any) => (
                  <li key={f.id} className="list-item" onContextMenu={(e) => openMenu(e, ref(course, { kind: "file", label: f.display_name, id: String(f.id) }))}>
                    <div className="grow"><div className="title">{f.display_name}</div><div className="muted small">{bytes(f.size)} · {formatDate(f.updated_at)}</div></div>
                    <button disabled={f.locked_for_user || downloading === f.id} onClick={async () => {
                      setDownloading(f.id);
                      try { const p = await api.download(f.id); setMsg({ tone: "ok", text: `저장했습니다: ${p}` }); } catch (e) { setMsg({ tone: "error", text: errorText(e) }); } finally { setDownloading(null); }
                    }}>{downloading === f.id ? "받는 중…" : f.locked_for_user ? "잠김" : "다운로드"}</button>
                  </li>
                ))}
              </ul>
            )}</Loading>
          )}
        </div>
      </div>
    </section>
  );
}

// ---------------- 주차학습 (모듈) ----------------
export function Modules({ course }: P) {
  const { openMenu } = useApp();
  const r = useLoad(() => api.canvas<any[]>("canvas_list_modules", { course_id: course.id }), [course.id]);
  const [page, setPage] = useState<any>(null);
  const [msg, setMsg] = useState("");
  const typeLabel: Record<string, string> = { Assignment: "과제", Page: "페이지", File: "파일", Discussion: "토론", Quiz: "퀴즈", ExternalUrl: "링크", ExternalTool: "외부 도구", SubHeader: "제목" };
  const kindOf = (t: string): CanvasRef["kind"] => (t === "Assignment" ? "assignment" : t === "Discussion" ? "discussion" : t === "File" ? "file" : t === "Quiz" ? "quiz" : t === "Page" ? "page" : "module_item");
  const openItem = async (m: any, it: any) => {
    if (it.completion_requirement?.type === "must_view") void canvasAction("module_item_read", { course_id: course.id, module_id: m.id, item_id: it.id }).catch(() => {});
    if (it.type === "Page" && it.page_url) {
      setPage({ loading: true });
      try { setPage(await api.canvas("canvas_get_page", { course_id: course.id, page_url: it.page_url })); } catch (e) { setPage({ error: errorText(e) }); }
    } else open(it.html_url);
  };
  return (
    <section className="card">
      <h2>🗂 주차학습</h2>
      {msg && <Banner tone="info" onClose={() => setMsg("")}>{msg}</Banner>}
      <Loading r={r} empty="모듈이 없습니다">{(list) => list.map((m: any) => (
        <div key={m.id} className="module">
          <h3>{m.name} {m.state === "completed" && <Badge tone="ok">완료</Badge>}{m.state === "locked" && <Badge tone="past">잠김</Badge>}</h3>
          <ul className="list compact">
            {(m.items ?? []).map((it: any) => it.type === "SubHeader" ? (
              <li key={it.id} className="list-item subheader">{it.title}</li>
            ) : (
              <li key={it.id} className="list-item clickable" onClick={() => void openItem(m, it)}
                onContextMenu={(e) => openMenu(e, ref(course, { kind: kindOf(it.type), label: it.title, id: it.type === "Page" ? it.page_url : String(it.content_id ?? it.id), htmlUrl: it.html_url, extra: { module_id: m.id, item_id: it.id } }))}>
                <Badge>{typeLabel[it.type] ?? it.type}</Badge>
                <span className="grow">{it.title}</span>
                {it.completion_requirement?.type === "must_mark_done" && (
                  <button className={it.completion_requirement.completed ? "chip on" : "chip"} onClick={async (e) => {
                    e.stopPropagation();
                    const done = !it.completion_requirement.completed;
                    try { await canvasAction("module_item_done", { course_id: course.id, module_id: m.id, item_id: it.id, done }); await r.reload(); }
                    catch (err) { setMsg(errorText(err)); }
                  }}>{it.completion_requirement.completed ? "✓ 완료함" : "완료로 표시"}</button>
                )}
                {it.completion_requirement && it.completion_requirement.type !== "must_mark_done" && it.completion_requirement.completed && <Badge tone="ok">✓</Badge>}
              </li>
            ))}
          </ul>
        </div>
      ))}</Loading>
      {page && (
        <Modal onClose={() => setPage(null)}>
          {page.loading ? <Spinner label="불러오는 중…" /> : page.error ? <Banner>{page.error}</Banner> : <><h2>{page.title}</h2><p className="pre">{page.body || "내용이 없습니다."}</p></>}
          <div className="row end">{page.html_url && <button onClick={() => open(page.html_url)}>Canvas에서 열기</button>}<button className="primary" onClick={() => setPage(null)}>닫기</button></div>
        </Modal>
      )}
    </section>
  );
}

// ---------------- 과제 및 평가 ----------------
function AssignmentRow({ a, course, onContext, onClick }: { a: any; course: Course; onContext: (e: React.MouseEvent, r: CanvasRef) => void; onClick?: () => void }) {
  const r = ref(course, { kind: "assignment", label: a.name, id: String(a.id), htmlUrl: a.html_url, dueAt: a.due_at });
  return (
    <li className="list-item clickable" onClick={onClick ?? (() => open(a.html_url))} onContextMenu={(e) => onContext(e, r)}>
      <div className="grow">
        <div className="title">{a.name}</div>
        <div className="muted small">{[a.points_possible != null ? `${a.points_possible}점` : "", ...(a.submission_types ?? []).map((t: string) => ({ online_upload: "파일", online_text_entry: "텍스트", online_url: "URL", online_quiz: "퀴즈", discussion_topic: "토론", external_tool: "외부 도구", on_paper: "오프라인", none: "제출 없음", media_recording: "미디어" } as Record<string, string>)[t] ?? t)].filter(Boolean).join(" · ")}</div>
      </div>
      <div className="right">
        <Due iso={a.due_at} />
        <span className="row">
          <Badge tone={isSubmitted(a) ? "ok" : "neutral"}>{assignmentStatus(a)}</Badge>
          {a.submission?.score != null && <Badge tone="accent">{a.submission.score}점</Badge>}
        </span>
      </div>
    </li>
  );
}

export function Assignments({ course }: P) {
  const { openMenu } = useApp();
  const [bucket, setBucket] = useState("");
  const r = useLoad(() => api.canvas<any[]>("canvas_list_assignments", { course_id: course.id, bucket: bucket || undefined }), [course.id, bucket]);
  const [detail, setDetail] = useState<any>(null);
  // A submission made in the app (or a background refresh) changes the status: reload the list.
  useEffect(() => {
    const un = listen("canvas-updated", () => { void r.reload(); });
    return () => { void un.then((f) => f()); };
  }, [r.reload]);
  return (
    <section className="card">
      <div className="row space">
        <h2>📝 과제 및 평가</h2>
        <select value={bucket} onChange={(e) => setBucket(e.target.value)} aria-label="과제 필터">
          <option value="">전체</option><option value="upcoming">다가오는</option><option value="unsubmitted">미제출</option><option value="overdue">기한 지남</option><option value="past">지난 과제</option>
        </select>
      </div>
      <p className="muted small">과제를 우클릭하면 AI 채팅에 추가, 분석, 제출을 할 수 있습니다.</p>
      <Loading r={r} empty="해당하는 과제가 없습니다">{(list) => (
        <ul className="list">{list.map((a: any) => <AssignmentRow key={a.id} a={a} course={course} onContext={openMenu} onClick={() => setDetail(a)} />)}</ul>
      )}</Loading>
      {detail && <AssignmentDetail course={course} a={detail} onClose={() => { setDetail(null); void r.reload(); }} />}
    </section>
  );
}

function AssignmentDetail({ course, a, onClose }: P & { a: any; onClose: () => void }) {
  const { askAI, openSubmit } = useApp();
  const d = useLoad(() => api.canvas<any>("canvas_get_assignment", { course_id: course.id, assignment_id: a.id }), [a.id]);
  const mine = useLoad(() => api.canvas<any>("canvas_my_submission", { course_id: course.id, assignment_id: a.id }), [a.id]);
  const [comment, setComment] = useState("");
  const [msg, setMsg] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const r = ref(course, { kind: "assignment", label: a.name, id: String(a.id), htmlUrl: a.html_url, dueAt: a.due_at });
  return (
    <Modal onClose={onClose}>
      <div className="row space">
        <h2>{a.name}</h2>
        <div className="row">
          <button onClick={() => askAI("첨부한 과제를 분석해줘. 해야 할 일, 채점 기준(루브릭), 제출 방식과 허용 형식, 배점 비중, 마감까지 남은 시간을 정리하고 작업 순서를 제안해줘.", { send: true, refs: [r] })}>✨ AI 분석</button>
          <button className="primary" onClick={() => { onClose(); openSubmit(r); }}>📤 제출하기</button>
        </div>
      </div>
      <Loading r={d}>{(x) => (
        <>
          <div className="row wrap">
            <Due iso={x.due_at} />
            {x.points_possible != null && <Badge>{x.points_possible}점</Badge>}
            {x.allowed_attempts > 0 && <Badge>제출 {x.allowed_attempts}회까지</Badge>}
            {x.allowed_extensions?.length > 0 && <Badge tone="warn">{x.allowed_extensions.join(", ")}</Badge>}
            {x.locked_for_user && <Badge tone="past">잠김</Badge>}
          </div>
          <p className="pre">{x.description || "설명이 없습니다."}</p>
          {x.rubric?.length > 0 && (
            <div>
              <h3>채점 기준 (루브릭)</h3>
              <table className="table">
                <thead><tr><th>기준</th><th>배점</th><th>등급</th></tr></thead>
                <tbody>{x.rubric.map((c: any, i: number) => (
                  <tr key={i}><td>{c.description}{c.long_description && <div className="muted small">{c.long_description}</div>}</td><td>{c.points}</td>
                    <td className="small">{(c.ratings ?? []).map((rt: any) => `${rt.description} (${rt.points})`).join(" · ")}</td></tr>
                ))}</tbody>
              </table>
            </div>
          )}
        </>
      )}</Loading>
      <h3>내 제출</h3>
      <Loading r={mine}>{(m) => (
        <>
          <div className="row wrap">
            <Badge tone={isSubmitted({ submission: m }) ? "ok" : "neutral"}>{assignmentStatus({ submission: m })}</Badge>
            {m.attempt > 0 && <Badge>{m.attempt}번째 제출</Badge>}
            {m.score != null && <Badge tone="accent">{m.score}점</Badge>}
            {m.submitted_at && <span className="muted small">{formatDate(m.submitted_at)}</span>}
          </div>
          {m.attachments?.length > 0 && <p className="small">📎 {m.attachments.map((f: any) => f.display_name).join(", ")}</p>}
          {m.url && <p className="small break">🔗 {m.url}</p>}
          {m.history?.length > 1 && <p className="muted small">제출 이력: {m.history.map((h: any) => `${h.attempt ?? "-"}차 ${formatDate(h.submitted_at)}${h.score != null ? ` (${h.score}점)` : ""}`).join(" · ")}</p>}
          {isSubmitted({ submission: m }) && <SubmissionContent course={course} assignmentId={String(a.id)} attempts={(m.history ?? []).map((h: any) => h.attempt).filter((x: any) => x != null)} aRef={r} />}
          {(m.comments ?? []).length > 0 && (
            <div className="comments">{m.comments.map((c: any, i: number) => (
              <div key={i} className="message"><div className="row space"><strong>{c.author_name}</strong><span className="muted small">{formatDate(c.created_at)}</span></div><p className="pre">{c.comment}</p></div>
            ))}</div>
          )}
        </>
      )}</Loading>
      {msg && <Banner tone={msg.tone} onClose={() => setMsg(null)}>{msg.text}</Banner>}
      <div className="row">
        <input className="grow" value={comment} onChange={(e) => setComment(e.target.value)} placeholder="제출물에 코멘트 남기기" />
        <button disabled={!comment.trim()} onClick={async () => {
          try { await canvasAction("submission_comment", { course_id: course.id, assignment_id: a.id, text: comment }); setComment(""); setMsg({ tone: "ok", text: "코멘트를 남겼습니다." }); await mine.reload(); }
          catch (e) { setMsg({ tone: "error", text: errorText(e) }); }
        }}>남기기</button>
      </div>
      <div className="row end"><button onClick={() => open(a.html_url)}>Canvas에서 열기</button><button onClick={onClose}>닫기</button></div>
    </Modal>
  );
}

// ---------------- 퀴즈 ----------------
export function Quizzes({ course }: P) {
  const { openMenu } = useApp();
  const r = useLoad(() => api.canvas<any[]>("canvas_quizzes", { course_id: course.id }), [course.id]);
  return (
    <section className="card">
      <h2>❓ 퀴즈·설문</h2>
      <p className="muted small">응시는 Canvas에서 합니다. 우클릭하면 AI에게 준비 방법을 물어볼 수 있습니다.</p>
      <Loading r={r} empty="퀴즈가 없습니다">{(list) => (
        <ul className="list">{list.map((q: any) => (
          <li key={q.id} className="list-item clickable" onClick={() => open(q.html_url)} onContextMenu={(e) => openMenu(e, ref(course, { kind: "quiz", label: q.title, id: String(q.id), htmlUrl: q.html_url, dueAt: q.due_at }))}>
            <div className="grow">
              <div className="title">{q.title} {q.quiz_type === "survey" || q.quiz_type === "graded_survey" ? <Badge>설문</Badge> : null}</div>
              <div className="muted small">{q.question_count != null && `${q.question_count}문항 · `}{q.time_limit ? `${q.time_limit}분 · ` : ""}{q.allowed_attempts > 0 ? `${q.allowed_attempts}회 응시` : "응시 제한 없음"}{q.points_possible != null && ` · ${q.points_possible}점`}</div>
            </div>
            <div className="right"><Due iso={q.due_at} />{q.locked_for_user && <Badge tone="past">잠김</Badge>}</div>
          </li>
        ))}</ul>
      )}</Loading>
    </section>
  );
}

// ---------------- 사용자 및 그룹 ----------------
export function People({ course }: P) {
  const people = useLoad(() => api.canvas<any[]>("canvas_course_people", { course_id: course.id }), [course.id]);
  const groups = useLoad(() => api.canvas<any[]>("canvas_groups"), []);
  const [members, setMembers] = useState<Record<string, any[]>>({});
  const role = (p: any) => (p.roles ?? []).includes("TeacherEnrollment") ? "교수" : (p.roles ?? []).includes("TaEnrollment") ? "조교" : "학생";
  const mine = (groups.data ?? []).filter((g) => String(g.course_id) === String(course.id));
  return (
    <>
      <section className="card">
        <h2>👥 내 그룹</h2>
        <Loading r={groups}>{() => mine.length === 0 ? <Empty title="이 강의에서 속한 그룹이 없습니다" /> : (
          <ul className="list">{mine.map((g) => (
            <li key={g.id} className="list-item column">
              <button className="row-button" onClick={async () => { if (!members[g.id]) { try { const m = await api.canvas<any[]>("canvas_group_members", { group_id: g.id }); setMembers((o) => ({ ...o, [g.id]: m })); } catch { /* ignore */ } } }}>
                <div className="grow"><div className="title">{g.name}</div><div className="muted small">{g.members_count}명</div></div><span className="chev">⌄</span>
              </button>
              {members[g.id] && <div className="people-pick">{members[g.id].map((m) => <span key={m.id} className="chip">{m.name}</span>)}</div>}
            </li>
          ))}</ul>
        )}</Loading>
      </section>
      <section className="card">
        <h2>사용자</h2>
        <Loading r={people} empty="사용자 목록을 볼 수 없습니다">{(list) => {
          const order = ["교수", "조교", "학생"];
          return order.map((ro) => {
            const items = list.filter((p: any) => role(p) === ro);
            if (!items.length) return null;
            return <div key={ro}><h3>{ro} {items.length}</h3><div className="people-pick">{items.map((p: any) => <span key={p.id} className="chip">{p.name}</span>)}</div></div>;
          });
        }}</Loading>
      </section>
    </>
  );
}

// ---------------- 성적 ----------------
export function Grades({ course }: P) {
  const { askAI } = useApp();
  const groups = useLoad(() => api.canvas<any[]>("canvas_assignment_groups", { course_id: course.id }), [course.id]);
  const [whatIf, setWhatIf] = useState<Record<string, string>>({});
  const [result, setResult] = useState<any>(null);
  const [msg, setMsg] = useState("");
  const total = (gs: any[]) => {
    const weighted = gs.some((g) => g.group_weight);
    let earned = 0, possible = 0, wsum = 0, wtotal = 0;
    for (const g of gs) {
      let e = 0, p = 0;
      for (const a of g.assignments ?? []) {
        const s = a.submission;
        const score = whatIf[a.id] !== undefined && whatIf[a.id] !== "" ? Number(whatIf[a.id]) : s?.score;
        if (score == null || a.omit_from_final_grade || s?.excused) continue;
        e += Number(score); p += Number(a.points_possible ?? 0);
      }
      earned += e; possible += p;
      if (weighted && p > 0) { wsum += (e / p) * (g.group_weight ?? 0); wtotal += g.group_weight ?? 0; }
    }
    return weighted ? (wtotal ? (wsum / wtotal) * 100 : null) : possible ? (earned / possible) * 100 : null;
  };
  return (
    <section className="card">
      <div className="row space">
        <h2>📊 성적</h2>
        <div className="row">
          <button onClick={() => askAI("첨부한 강의의 현재 성적을 평가 항목 비중별로 분석하고, 남은 과제에서 어느 정도 받아야 목표 점수에 도달하는지 알려줘.", { refs: [{ kind: "course", label: course.name, courseId: course.id }] })}>✨ AI 분석</button>
          {course.computed_current_score != null && <Badge tone="accent">Canvas 현재 점수 {course.computed_current_score}점</Badge>}
        </div>
      </div>
      <p className="muted small">점수 칸에 가상의 점수를 넣으면 앱 안에서 예상 점수를 계산합니다(Canvas에는 저장되지 않음). Canvas의 성적 예측(What-if)에 저장하려면 '예측 저장'을 누르세요.</p>
      {msg && <Banner tone="info" onClose={() => setMsg("")}>{msg}</Banner>}
      <Loading r={groups} empty="성적 항목이 없습니다">{(gs) => {
        const t = total(gs);
        return (
          <>
            <p><strong>예상 점수: {t == null ? "—" : `${t.toFixed(1)}점`}</strong> {Object.keys(whatIf).length > 0 && <button className="link small" onClick={() => setWhatIf({})}>가상 점수 지우기</button>}</p>
            {gs.map((g: any) => (
              <div key={g.id} className="grade-group">
                <h3>{g.name} {g.group_weight ? <Badge>{g.group_weight}%</Badge> : null}</h3>
                <table className="table">
                  <thead><tr><th>과제</th><th>점수</th><th>가상 점수</th></tr></thead>
                  <tbody>{(g.assignments ?? []).map((a: any) => (
                    <tr key={a.id}>
                      <td>{a.name}<div className="muted small">{formatDate(a.due_at)}</div></td>
                      <td>{a.submission?.excused ? "면제" : a.submission?.score != null ? `${a.submission.score} / ${a.points_possible ?? "-"}` : `— / ${a.points_possible ?? "-"}`}</td>
                      <td className="row">
                        <input className="score-input" inputMode="decimal" value={whatIf[a.id] ?? ""} placeholder={a.submission?.score != null ? "" : "?"} onChange={(e) => setWhatIf((o) => ({ ...o, [a.id]: e.target.value.replace(/[^0-9.]/g, "") }))} aria-label={`${a.name} 가상 점수`} />
                        {a.submission?.id && whatIf[a.id] && (
                          <button className="link small" onClick={async () => {
                            try { const r = await canvasAction("what_if", { submission_id: a.submission.id, score: Number(whatIf[a.id]) }); setResult(r); setMsg("Canvas 성적 예측에 저장했습니다."); }
                            catch (e) { setMsg(errorText(e)); }
                          }}>예측 저장</button>
                        )}
                      </td>
                    </tr>
                  ))}</tbody>
                </table>
              </div>
            ))}
            {result && <p className="muted small">Canvas 계산 결과: {JSON.stringify((result.grades ?? result)?.[0]?.current ?? result).slice(0, 200)}</p>}
            <div className="row end"><button className="ghost" onClick={async () => { try { await canvasAction("what_if_reset", { course_id: course.id }); setMsg("Canvas 성적 예측을 초기화했습니다."); setResult(null); } catch (e) { setMsg(errorText(e)); } }}>Canvas 예측 초기화</button></div>
          </>
        );
      }}</Loading>
    </section>
  );
}

// ---------------- 페이지 ----------------
export function Pages({ course }: P) {
  const { openMenu } = useApp();
  const r = useLoad(() => api.canvas<any>("canvas_api_get", { path: `courses/${course.id}/pages`, query: { sort: "title" } }), [course.id]);
  const [page, setPage] = useState<any>(null);
  return (
    <section className="card">
      <h2>📄 페이지</h2>
      <Loading r={r}>{(list) => (Array.isArray(list) && list.length ? (
        <ul className="list">{list.map((p: any) => (
          <li key={p.page_id ?? p.url} className="list-item clickable" onContextMenu={(e) => openMenu(e, ref(course, { kind: "page", label: p.title, id: p.url, htmlUrl: p.html_url }))}
            onClick={async () => { setPage({ loading: true }); try { setPage(await api.canvas("canvas_get_page", { course_id: course.id, page_url: p.url })); } catch (e) { setPage({ error: errorText(e) }); } }}>
            <span className="grow">{p.title}</span><span className="muted small">{formatDate(p.updated_at)}</span>
          </li>
        ))}</ul>
      ) : <Empty title="페이지가 없습니다" />)}</Loading>
      {page && (
        <Modal onClose={() => setPage(null)}>
          {page.loading ? <Spinner /> : page.error ? <Banner>{page.error}</Banner> : <><h2>{page.title}</h2><Markdown text={page.body || "내용이 없습니다."} /></>}
          <div className="row end"><button className="primary" onClick={() => setPage(null)}>닫기</button></div>
        </Modal>
      )}
    </section>
  );
}

// ---------------- 내 제출물 내용 ----------------
function SubmissionContent({ course, assignmentId, attempts, aRef }: P & { assignmentId: string; attempts: number[]; aRef: CanvasRef }) {
  const { askAI } = useApp();
  const [attempt, setAttempt] = useState<number | "">("");
  const [data, setData] = useState<any>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  async function load(at: number | "") {
    setLoading(true); setError("");
    try { setData(await api.canvas<any>("canvas_read_submission", { course_id: course.id, assignment_id: assignmentId, ...(at ? { attempt: at } : {}) })); }
    catch (e) { setError(errorText(e)); } finally { setLoading(false); }
  }
  return (
    <div className="submission-content">
      <div className="row wrap">
        <button onClick={() => void load(attempt)} disabled={loading}>📄 제출한 내용 보기</button>
        {attempts.length > 1 && (
          <select aria-label="제출 차수" value={attempt} onChange={(e) => { const v = e.target.value ? Number(e.target.value) : ""; setAttempt(v); if (data) void load(v); }}>
            <option value="">최신 제출</option>
            {attempts.map((n) => <option key={n} value={n}>{n}차 제출</option>)}
          </select>
        )}
        <button onClick={() => askAI(`첨부한 과제에 내가 ${attempt ? `${attempt}차로` : ""} 제출한 내용을 canvas_read_submission${attempt ? `(attempt=${attempt})` : ""}으로 읽고, 무엇을 냈는지와 받은 피드백·개선할 점을 정리해줘.`, { send: true, refs: [aRef] })}>✨ AI로 제출물 검토</button>
      </div>
      {loading && <Spinner label="제출 파일을 읽는 중…" />}
      {error && <Banner>{error}</Banner>}
      {data && !loading && (
        <div className="stack">
          {data.submitted === false && <p className="muted">{data.note}</p>}
          {data.body && <details open><summary>제출한 글</summary><pre className="pre small code">{data.body}</pre></details>}
          {data.url && <p className="small break">🔗 {data.url}</p>}
          {(data.files ?? []).map((f: any) => (
            <details key={f.id} open={(data.files ?? []).length === 1}>
              <summary>📎 {f.display_name} <span className="muted small">{bytes(f.size)}</span></summary>
              {f.error ? <Banner tone="info">{f.error}</Banner> : <pre className="pre small code">{f.text}{f.note ? `\n\n… ${f.note}` : ""}</pre>}
            </details>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------- LearningX 강의자료실 ----------------
export function LearningX({ course, label, url }: P & { label: string; url?: string }) {
  const { openMenu, askAI } = useApp();
  const res = useLoad(() => api.canvas<any>("canvas_learningx_resources", { course_id: course.id }), [course.id]);
  const [msg, setMsg] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ title: string; text: string; note?: string } | null>(null);
  const items: any[] = res.data?.items ?? [];
  const groups = useMemo(() => {
    const m = new Map<string, any[]>();
    for (const it of items) { const k = it.section ?? "자료"; if (!m.has(k)) m.set(k, []); m.get(k)!.push(it); }
    return Array.from(m.entries());
  }, [items]);
  const mref = (it: any) => ref(course, { kind: "material", label: it.title, id: String(it.content_id), extra: { type: it.type } });
  return (
    <section className="card">
      <div className="row space">
        <h2>📁 {label}</h2>
        <div className="row">
          <button onClick={() => void res.reload()} disabled={res.loading}>새로고침</button>
          {url && <button onClick={() => open(url)}>브라우저에서 열기</button>}
        </div>
      </div>
      <p className="muted small">LearningX 강의자료실을 앱에서 읽어옵니다. 우클릭하면 AI 채팅에 추가하거나 내려받을 수 있습니다.</p>
      {msg && <Banner tone={msg.tone} onClose={() => setMsg(null)}>{msg.text}</Banner>}
      {res.loading ? <Spinner label="LearningX에 연결하는 중…" /> : res.error ? (
        <Banner>{res.error} — {url ? "브라우저에서 열어 확인하세요." : ""}</Banner>
      ) : items.length === 0 ? (
        <>
          <Empty title="자료를 찾지 못했습니다" icon="📂" />
          {res.data?.raw && <details><summary className="muted small">LearningX 응답 보기</summary><pre className="pre small">{res.data.raw}</pre></details>}
        </>
      ) : groups.map(([section, list]) => (
        <div key={section} className="module">
          <h3>{section}</h3>
          <ul className="list">
            {list.map((it: any) => (
              <li key={it.content_id} className="list-item" onContextMenu={(e) => openMenu(e, mref(it))}>
                <div className="grow">
                  <div className="title">{it.title}</div>
                  <div className="muted small">{[it.type, it.file_name, typeof it.size === "number" ? bytes(it.size) : null, it.created_at ? formatDate(it.created_at) : null].filter(Boolean).join(" · ")}</div>
                </div>
                <button disabled={busy === it.content_id} onClick={async () => {
                  setBusy(it.content_id);
                  try { const r = await api.canvas<any>("canvas_learningx_read", { course_id: course.id, content_id: it.content_id }); setPreview({ title: it.title, text: r.text, note: r.note }); }
                  catch (e) { setMsg({ tone: "error", text: errorText(e) }); } finally { setBusy(null); }
                }}>내용 보기</button>
                <button onClick={() => askAI("첨부한 강의자료를 canvas_learningx_read로 읽고 핵심 내용을 정리해줘.", { send: true, refs: [mref(it)] })}>✨ 요약</button>
                <button disabled={busy === it.content_id} onClick={async () => {
                  setBusy(it.content_id);
                  try { const p = await api.lxDownload(course.id, it.content_id); setMsg({ tone: "ok", text: `저장했습니다: ${p}` }); }
                  catch (e) { setMsg({ tone: "error", text: errorText(e) }); } finally { setBusy(null); }
                }}>다운로드</button>
              </li>
            ))}
          </ul>
        </div>
      ))}
      {preview && (
        <Modal onClose={() => setPreview(null)}>
          <h2>{preview.title}</h2>
          <pre className="pre small code">{preview.text}{preview.note ? `\n\n… ${preview.note}` : ""}</pre>
          <div className="row end"><button onClick={() => setPreview(null)}>닫기</button></div>
        </Modal>
      )}
    </section>
  );
}
