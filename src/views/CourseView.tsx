import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useApp } from "../App";
import { Banner, open, Spinner } from "../components";
import { useLoad } from "../shared";
import { Announcements, Assignments, Discussions, Files, Grades, Home, LearningX, Modules, Pages, People, Quizzes, Syllabus } from "./CoursePanels";

export type Course = { id: string; name: string; course_code?: string; term?: string; computed_current_score?: number; computed_current_grade?: string; html_url?: string };
type Tab = { id: string; label: string; type?: string; html_url?: string; full_url?: string };

const icons: Record<string, string> = {
  home: "🏠", syllabus: "📋", announcements: "📢", files: "📁", discussions: "💬", modules: "🗂", assignments: "📝",
  quizzes: "❓", people: "👥", grades: "📊", pages: "📄", outcomes: "🎯", conferences: "🎥", collaborations: "🤝",
};
const built = new Set(["home", "syllabus", "announcements", "files", "discussions", "modules", "assignments", "quizzes", "people", "grades", "pages"]);

/** Fallback when the tabs API is unavailable. */
const defaultTabs: Tab[] = [
  { id: "home", label: "홈" }, { id: "syllabus", label: "수업 계획서" }, { id: "announcements", label: "공지" },
  { id: "files", label: "강의자료실" }, { id: "discussions", label: "게시판" }, { id: "modules", label: "주차학습" },
  { id: "assignments", label: "과제 및 평가" }, { id: "quizzes", label: "퀴즈" }, { id: "people", label: "사용자 및 그룹" }, { id: "grades", label: "성적" },
];

export default function CourseView({ course, back }: { course: Course; back: () => void }) {
  const { askAI } = useApp();
  const tabs = useLoad(() => api.canvas<Tab[]>("canvas_course_tabs", { course_id: course.id }), [course.id]);
  const list = useMemo(() => (tabs.data && tabs.data.length ? tabs.data : tabs.error ? defaultTabs : []), [tabs.data, tabs.error]);
  const [active, setActive] = useState<string>("home");
  useEffect(() => { if (list.length && !list.some((t) => t.id === active)) setActive(list[0].id); }, [list, active]);
  const tab = list.find((t) => t.id === active);

  return (
    <div className="page">
      <header className="topbar">
        <div className="row">
          <button className="ghost" onClick={back} aria-label="강의 목록으로">←</button>
          <h1>{course.name}</h1>
        </div>
        <div className="row">
          <button onClick={() => askAI("첨부한 강의의 남은 과제, 최근 공지, 현재 성적을 정리해줘", { refs: [{ kind: "course", label: course.name, courseId: course.id, htmlUrl: course.html_url }] })}>✨ AI로 정리</button>
          <button onClick={() => open(course.html_url)}>Canvas에서 열기</button>
        </div>
      </header>
      <div className="course-layout">
        <nav className="course-nav" aria-label="강의 메뉴">
          {course.term && <div className="muted small course-term">{course.term}</div>}
          {tabs.loading ? <Spinner /> : list.map((t) => (
            <button key={t.id} className={"course-tab" + (active === t.id ? " active" : "")} onClick={() => setActive(t.id)} aria-current={active === t.id ? "page" : undefined}>
              <span aria-hidden>{icons[t.id] ?? (isMaterials(t) ? "📁" : t.type === "external" ? "🔗" : "•")}</span>
              <span className="grow">{t.label}</span>
              {!built.has(t.id) && !isMaterials(t) && <span className="muted small" aria-label="브라우저에서 열림">↗</span>}
            </button>
          ))}
          {tabs.error && <p className="muted small">학교 메뉴 정보를 불러오지 못해 기본 메뉴를 보여줍니다.</p>}
        </nav>
        <div className="content course-content">
          {tab && <TabPanel tab={tab} course={course} />}
        </div>
      </div>
    </div>
  );
}

function TabPanel({ tab, course }: { tab: Tab; course: Course }) {
  const c = course;
  switch (tab.id) {
    case "home": return <Home course={c} />;
    case "syllabus": return <Syllabus course={c} />;
    case "announcements": return <Announcements course={c} />;
    case "files": return <Files course={c} />;
    case "discussions": return <Discussions course={c} />;
    case "modules": return <Modules course={c} />;
    case "assignments": return <Assignments course={c} />;
    case "quizzes": return <Quizzes course={c} />;
    case "people": return <People course={c} />;
    case "grades": return <Grades course={c} />;
    case "pages": return <Pages course={c} />;
    default:
      return isMaterials(tab) ? <LearningX course={c} label={tab.label} url={tab.full_url || tab.html_url} /> : <External tab={tab} />;
  }
}

/** LearningX "강의자료실" style tools are read through the LearningX API. */
const isMaterials = (t: Tab) => t.id.startsWith("context_external_tool_") && /강의\s*자료|자료실|learning\s*x|resource/i.test(t.label);

/** School-added tools (attendance, learning analytics, …) cannot be read through the API. */
function External({ tab }: { tab: Tab }) {
  const url = tab.full_url || tab.html_url;
  return (
    <section className="card">
      <h2>🔗 {tab.label}</h2>
      <p className="muted">이 메뉴는 학교에서 연결한 외부 도구라 Canvas API로 내용을 가져올 수 없습니다. 브라우저에서 열어 확인하세요.</p>
      {url ? <div className="row"><button className="primary" onClick={() => open(url)}>브라우저에서 열기</button></div> : <Banner tone="info">열 수 있는 주소가 없습니다.</Banner>}
    </section>
  );
}
