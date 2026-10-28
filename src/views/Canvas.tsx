import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import { useApp } from "../App";
import { Badge, Banner, Empty, open, Spinner } from "../components";
import { Due, useLoad } from "../shared";
import { formatDate, isSubmitted, submissionLabel } from "../util";
import CourseView, { Course } from "./CourseView";

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
  return course ? <CourseView course={course} back={() => setCourse(null)} /> : <Dashboard onCourse={setCourse} />;
}

function Dashboard({ onCourse }: { onCourse: (c: Course) => void }) {
  const { askAI, openMenu } = useApp();
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
  const courseOf = (a: any) => String(a.course_id ?? "");
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
          <div className="row space"><h2>다가오는 과제</h2><span className="muted small">우클릭: AI 채팅에 추가 · 분석 · 제출</span></div>
          {upcoming.loading ? <Spinner label="불러오는 중…" /> : upcoming.error ? <Banner>{upcoming.error}</Banner> : work.length === 0 ? (
            <Empty title="다가오는 과제가 없습니다" icon="🎉" />
          ) : (
            <ul className="list">
              {work.map((a) => (
                <li key={a.id} className="list-item clickable" onClick={() => open(a.html_url)}
                  onContextMenu={(e) => openMenu(e, { kind: "assignment", label: a.name, id: String(a.id), courseId: courseOf(a), courseName: a.context_name, htmlUrl: a.html_url, dueAt: a.due_at })}>
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
                <li key={c.id} className="list-item clickable" onClick={() => onCourse(c)}
                  onContextMenu={(e) => openMenu(e, { kind: "course", label: c.name, courseId: String(c.id), htmlUrl: c.html_url })}>
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
                <li key={a.id} className="list-item column"
                  onContextMenu={(e) => openMenu(e, { kind: "announcement", label: a.title, id: String(a.id), courseId: String(a.context_code ?? "").replace("course_", ""), courseName: courseName(a.context_code), htmlUrl: a.html_url })}>
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

