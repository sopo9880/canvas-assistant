import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorText } from "../api";
import { useApp } from "../App";
import { Badge, Banner, Empty, open, Spinner } from "../components";
import { bytes, formatDate, isSubmitted, submissionLabel, timeLeft } from "../util";

type Course = { id: string; name: string; course_code?: string; term?: string; computed_current_score?: number; computed_current_grade?: string; html_url?: string };

function Due({ iso }: { iso?: string }) {
  const t = timeLeft(iso);
  if (!t) return <span className="muted">마감 없음</span>;
  return <span className="due"><Badge tone={t.tone}>{t.label}</Badge><span className="muted small">{formatDate(iso)}</span></span>;
}

function useLoad<T>(fn: () => Promise<T>, deps: unknown[]) {
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
  return { data, error, loading, reload };
}

export default function CanvasView() {
  const { info, go } = useApp();
  const [course, setCourse] = useState<Course | null>(null);
  const configured = !!(info?.hasToken && info.settings.canvasBaseUrl);

  if (!configured) {
    return (
      <div className="page">
        <header className="topbar"><h1>Canvas</h1></header>
        <div className="content">
          <Empty title="Canvas가 아직 연결되지 않았습니다" icon="📚">
            <p>설정에서 학교 Canvas 주소와 액세스 토큰을 입력하면 과제, 공지, 성적, 파일을 여기서 볼 수 있습니다.</p>
            <button className="primary" onClick={() => go("settings")}>Canvas 연결하기</button>
          </Empty>
        </div>
      </div>
    );
  }
  return course ? <CourseDetail course={course} back={() => setCourse(null)} /> : <Dashboard onCourse={setCourse} />;
}

function Dashboard({ onCourse }: { onCourse: (c: Course) => void }) {
  const { askAI } = useApp();
  const courses = useLoad(() => api.canvas<Course[]>("canvas_list_courses"), []);
  const upcoming = useLoad(() => api.canvas<any>("canvas_upcoming"), []);
  const announcements = useLoad(() => api.canvas<any[]>("canvas_list_announcements", { days_back: 7 }), []);
  const [openAnn, setOpenAnn] = useState<string | null>(null);

  useEffect(() => {
    const un = listen("canvas-updated", () => { void announcements.reload(); void upcoming.reload(); });
    return () => { void un.then((f) => f()); };
  }, [announcements, upcoming]);

  const work: any[] = [];
  const seen = new Set<string>();
  for (const list of [upcoming.data?.todo ?? [], upcoming.data?.upcoming_events ?? []]) {
    for (const item of list) {
      const a = item.assignment;
      if (!a || seen.has(String(a.id))) continue;
      seen.add(String(a.id));
      work.push({ ...a, context_name: item.context_name });
    }
  }
  work.sort((x, y) => (x.due_at ?? "9999").localeCompare(y.due_at ?? "9999"));
  const courseName = (code?: string) => courses.data?.find((c) => `course_${c.id}` === code)?.name ?? "";
  const refresh = () => { void courses.reload(); void upcoming.reload(); void announcements.reload(); };

  return (
    <div className="page">
      <header className="topbar">
        <h1>Canvas</h1>
        <div className="row">
          <button onClick={() => askAI("이번 주 해야 할 일을 Canvas에서 확인해서 우선순위대로 정리해줘")}>✨ AI로 정리</button>
          <button onClick={refresh} title="새로고침">↻</button>
        </div>
      </header>
      <div className="content grid-2">
        <section className="card">
          <h2>다가오는 과제</h2>
          {upcoming.loading ? <Spinner label="불러오는 중…" /> : upcoming.error ? <Banner>{upcoming.error}</Banner> : work.length === 0 ? (
            <Empty title="다가오는 과제가 없습니다" icon="🎉" />
          ) : (
            <ul className="list">
              {work.map((a) => (
                <li key={a.id} className="list-item clickable" onClick={() => open(a.html_url)}>
                  <div className="grow">
                    <div className="title">{a.name}</div>
                    <div className="muted small">{a.context_name}{a.points_possible != null && ` · ${a.points_possible}점`}</div>
                  </div>
                  <div className="right">
                    <Due iso={a.due_at} />
                    {isSubmitted(a) ? <Badge tone="ok">{submissionLabel(a.submission?.workflow_state)}</Badge> : <Badge tone="neutral">미제출</Badge>}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="card">
          <h2>내 강의</h2>
          {courses.loading ? <Spinner label="불러오는 중…" /> : courses.error ? <Banner>{courses.error}</Banner> : (courses.data ?? []).length === 0 ? (
            <Empty title="수강 중인 강의가 없습니다" />
          ) : (
            <ul className="list">
              {courses.data!.map((c) => (
                <li key={c.id} className="list-item clickable" onClick={() => onCourse(c)}>
                  <div className="grow">
                    <div className="title">{c.name}</div>
                    <div className="muted small">{[c.course_code, c.term].filter(Boolean).join(" · ")}</div>
                  </div>
                  {c.computed_current_score != null && <Badge tone="accent">{c.computed_current_score}점{c.computed_current_grade ? ` (${c.computed_current_grade})` : ""}</Badge>}
                  <span className="chev" aria-hidden>›</span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="card span-2">
          <h2>최근 7일 공지</h2>
          {announcements.loading ? <Spinner label="불러오는 중…" /> : announcements.error ? <Banner>{announcements.error}</Banner> : (announcements.data ?? []).length === 0 ? (
            <Empty title="최근 공지가 없습니다" icon="📭" />
          ) : (
            <ul className="list">
              {announcements.data!.map((a) => (
                <li key={a.id} className="list-item column">
                  <button className="row-button" onClick={() => setOpenAnn(openAnn === a.id ? null : a.id)} aria-expanded={openAnn === a.id}>
                    <div className="grow">
                      <div className="title">{a.title}</div>
                      <div className="muted small">{courseName(a.context_code)} · {a.author} · {formatDate(a.posted_at)}</div>
                    </div>
                    <span className="chev" aria-hidden>{openAnn === a.id ? "⌃" : "⌄"}</span>
                  </button>
                  {openAnn === a.id && (
                    <div className="expand">
                      <p className="pre">{a.message}</p>
                      <div className="row">
                        <button onClick={() => open(a.html_url)}>Canvas에서 열기</button>
                        <button onClick={() => askAI(`이 공지를 요약하고 내가 해야 할 일이 있으면 알려줘: "${a.title}" (${courseName(a.context_code)})`)}>✨ AI에게 묻기</button>
                      </div>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}

type CourseTab = "assignments" | "announcements" | "modules" | "files" | "grades" | "discussions";

function CourseDetail({ course, back }: { course: Course; back: () => void }) {
  const { askAI } = useApp();
  const [tab, setTab] = useState<CourseTab>("assignments");
  const tabs: Array<[CourseTab, string]> = [["assignments", "과제"], ["announcements", "공지"], ["modules", "모듈"], ["files", "파일"], ["grades", "성적·제출"], ["discussions", "토론"]];
  return (
    <div className="page">
      <header className="topbar">
        <div className="row">
          <button className="ghost" onClick={back} aria-label="뒤로">←</button>
          <h1>{course.name}</h1>
        </div>
        <div className="row">
          <button onClick={() => askAI(`'${course.name}' 강의의 남은 과제와 최근 공지를 정리해줘`)}>✨ AI로 정리</button>
          <button onClick={() => open(course.html_url)}>Canvas에서 열기</button>
        </div>
      </header>
      <div className="tabs" role="tablist">
        {tabs.map(([id, label]) => (
          <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>{label}</button>
        ))}
      </div>
      <div className="content">
        {tab === "assignments" && <Assignments courseId={course.id} />}
        {tab === "announcements" && <CourseAnnouncements courseId={course.id} />}
        {tab === "modules" && <Modules courseId={course.id} />}
        {tab === "files" && <Files courseId={course.id} />}
        {tab === "grades" && <Submissions courseId={course.id} />}
        {tab === "discussions" && <Discussions courseId={course.id} />}
      </div>
    </div>
  );
}

function Assignments({ courseId }: { courseId: string }) {
  const [bucket, setBucket] = useState("upcoming");
  const r = useLoad(() => api.canvas<any[]>("canvas_list_assignments", { course_id: courseId, bucket: bucket || undefined }), [courseId, bucket]);
  const [detail, setDetail] = useState<any>(null);
  return (
    <section className="card">
      <div className="row space">
        <h2>과제</h2>
        <select value={bucket} onChange={(e) => setBucket(e.target.value)} aria-label="과제 필터">
          <option value="upcoming">다가오는</option><option value="unsubmitted">미제출</option><option value="overdue">기한 지남</option>
          <option value="past">지난 과제</option><option value="">전체</option>
        </select>
      </div>
      {r.loading ? <Spinner /> : r.error ? <Banner>{r.error}</Banner> : (r.data ?? []).length === 0 ? <Empty title="해당하는 과제가 없습니다" /> : (
        <ul className="list">
          {r.data!.map((a) => (
            <li key={a.id} className="list-item clickable" onClick={async () => {
              setDetail({ loading: true, name: a.name });
              try { setDetail(await api.canvas("canvas_get_assignment", { course_id: courseId, assignment_id: a.id })); } catch (e) { setDetail({ error: errorText(e) }); }
            }}>
              <div className="grow">
                <div className="title">{a.name}</div>
                <div className="muted small">{a.points_possible != null && `${a.points_possible}점 · `}{(a.submission_types ?? []).join(", ")}</div>
              </div>
              <div className="right">
                <Due iso={a.due_at} />
                <Badge tone={isSubmitted(a) ? "ok" : "neutral"}>{submissionLabel(a.submission?.workflow_state, a.submission ?? {}) || "—"}</Badge>
                {a.submission?.score != null && <Badge tone="accent">{a.submission.score}점</Badge>}
              </div>
            </li>
          ))}
        </ul>
      )}
      {detail && (
        <div className="modal" role="dialog" aria-modal onClick={() => setDetail(null)}>
          <div className="modal-body" onClick={(e) => e.stopPropagation()}>
            {detail.loading ? <Spinner label="불러오는 중…" /> : detail.error ? <Banner>{detail.error}</Banner> : (
              <>
                <h2>{detail.name}</h2>
                <div className="row wrap"><Due iso={detail.due_at} />{detail.points_possible != null && <Badge>{detail.points_possible}점</Badge>}</div>
                <p className="pre">{detail.description || "설명이 없습니다."}</p>
              </>
            )}
            <div className="row end">
              {detail.html_url && <button onClick={() => open(detail.html_url)}>Canvas에서 열기</button>}
              <button className="primary" onClick={() => setDetail(null)}>닫기</button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function CourseAnnouncements({ courseId }: { courseId: string }) {
  const r = useLoad(() => api.canvas<any[]>("canvas_list_announcements", { course_ids: [courseId], days_back: 120 }), [courseId]);
  return (
    <section className="card">
      <h2>공지 (최근 120일)</h2>
      {r.loading ? <Spinner /> : r.error ? <Banner>{r.error}</Banner> : (r.data ?? []).length === 0 ? <Empty title="공지가 없습니다" /> : (
        <ul className="list">
          {r.data!.map((a) => (
            <li key={a.id} className="list-item column">
              <div className="row space"><div className="title">{a.title}</div><span className="muted small">{formatDate(a.posted_at)}</span></div>
              <p className="pre muted">{a.message}</p>
              <button className="link" onClick={() => open(a.html_url)}>Canvas에서 열기 →</button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function Modules({ courseId }: { courseId: string }) {
  const r = useLoad(() => api.canvas<any[]>("canvas_list_modules", { course_id: courseId }), [courseId]);
  const [page, setPage] = useState<any>(null);
  return (
    <section className="card">
      <h2>모듈</h2>
      {r.loading ? <Spinner /> : r.error ? <Banner>{r.error}</Banner> : (r.data ?? []).length === 0 ? <Empty title="모듈이 없습니다" /> : (
        r.data!.map((m) => (
          <div key={m.id} className="module">
            <h3>{m.name}</h3>
            <ul className="list compact">
              {(m.items ?? []).map((it: any) => (
                <li key={it.id} className="list-item clickable" onClick={async () => {
                  if (it.type === "Page" && it.page_url) {
                    setPage({ loading: true });
                    try { setPage(await api.canvas("canvas_get_page", { course_id: courseId, page_url: it.page_url })); } catch (e) { setPage({ error: errorText(e) }); }
                  } else open(it.html_url);
                }}>
                  <Badge>{({ Assignment: "과제", Page: "페이지", File: "파일", Discussion: "토론", Quiz: "퀴즈", ExternalUrl: "링크", SubHeader: "제목" } as Record<string, string>)[it.type] ?? it.type}</Badge>
                  <span className="grow">{it.title}</span>
                </li>
              ))}
            </ul>
          </div>
        ))
      )}
      {page && (
        <div className="modal" role="dialog" aria-modal onClick={() => setPage(null)}>
          <div className="modal-body wide" onClick={(e) => e.stopPropagation()}>
            {page.loading ? <Spinner label="불러오는 중…" /> : page.error ? <Banner>{page.error}</Banner> : (
              <><h2>{page.title}</h2><p className="pre">{page.body || "내용이 없습니다."}</p></>
            )}
            <div className="row end">
              {page.html_url && <button onClick={() => open(page.html_url)}>Canvas에서 열기</button>}
              <button className="primary" onClick={() => setPage(null)}>닫기</button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function Files({ courseId }: { courseId: string }) {
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const r = useLoad(() => api.canvas<any[]>("canvas_list_files", { course_id: courseId, search: query || undefined }), [courseId, query]);
  const [status, setStatus] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const [downloading, setDownloading] = useState<string | null>(null);
  return (
    <section className="card">
      <div className="row space">
        <h2>파일</h2>
        <form className="row" onSubmit={(e) => { e.preventDefault(); setQuery(search.trim()); }}>
          <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="파일 이름 검색 (2자 이상)" aria-label="파일 검색" />
          <button type="submit">검색</button>
        </form>
      </div>
      {status && <Banner tone={status.tone} onClose={() => setStatus(null)}>{status.text}</Banner>}
      {r.loading ? <Spinner /> : r.error ? <Banner>{r.error}</Banner> : (r.data ?? []).length === 0 ? <Empty title="파일이 없거나 접근 권한이 없습니다" /> : (
        <ul className="list">
          {r.data!.map((f) => (
            <li key={f.id} className="list-item">
              <div className="grow">
                <div className="title">{f.display_name}</div>
                <div className="muted small">{bytes(f.size)} · {formatDate(f.updated_at)}</div>
              </div>
              <button disabled={f.locked_for_user || downloading === f.id} onClick={async () => {
                setDownloading(f.id);
                try { const path = await api.download(f.id); setStatus({ tone: "ok", text: `저장했습니다: ${path}` }); }
                catch (e) { setStatus({ tone: "error", text: errorText(e) }); }
                finally { setDownloading(null); }
              }}>{downloading === f.id ? "받는 중…" : f.locked_for_user ? "잠김" : "다운로드"}</button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function Submissions({ courseId }: { courseId: string }) {
  const r = useLoad(() => api.canvas<any[]>("canvas_list_submissions", { course_id: courseId }), [courseId]);
  const list = (r.data ?? []).slice().sort((a, b) => (b.graded_at ?? b.submitted_at ?? "").localeCompare(a.graded_at ?? a.submitted_at ?? ""));
  return (
    <section className="card">
      <h2>성적·제출 현황</h2>
      {r.loading ? <Spinner /> : r.error ? <Banner>{r.error}</Banner> : list.length === 0 ? <Empty title="제출 기록이 없습니다" /> : (
        <table className="table">
          <thead><tr><th>과제</th><th>상태</th><th>점수</th><th>마감</th></tr></thead>
          <tbody>
            {list.map((s) => (
              <tr key={s.assignment_id}>
                <td>{s.assignment_name}{s.comments?.length ? <div className="muted small">💬 {s.comments[s.comments.length - 1].comment}</div> : null}</td>
                <td>{submissionLabel(s.workflow_state, s)}</td>
                <td>{s.score != null ? `${s.score}${s.points_possible != null ? ` / ${s.points_possible}` : ""}` : s.grade ?? "—"}</td>
                <td className="muted small">{formatDate(s.due_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function Discussions({ courseId }: { courseId: string }) {
  const r = useLoad(() => api.canvas<any[]>("canvas_list_discussions", { course_id: courseId }), [courseId]);
  return (
    <section className="card">
      <h2>토론</h2>
      {r.loading ? <Spinner /> : r.error ? <Banner>{r.error}</Banner> : (r.data ?? []).length === 0 ? <Empty title="토론이 없습니다" /> : (
        <ul className="list">
          {r.data!.map((d) => (
            <li key={d.id} className="list-item clickable" onClick={() => open(d.html_url)}>
              <div className="grow">
                <div className="title">{d.title}</div>
                <div className="muted small">댓글 {d.discussion_subentry_count ?? 0}개{d.unread_count ? ` · 안 읽음 ${d.unread_count}` : ""} · {formatDate(d.last_reply_at ?? d.posted_at)}</div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

