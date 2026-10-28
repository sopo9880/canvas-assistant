// Dev-only UI preview with a mocked Tauri backend (not part of the app build).
import { mockIPC } from "@tauri-apps/api/mocks";
const now = Date.now();
const iso = (h: number) => new Date(now + h * 3600_000).toISOString();
const view = new URLSearchParams(location.search).get("view") ?? "chat";
const loggedIn = new URLSearchParams(location.search).get("login") !== "0";
const courses = [
  { id: "101", name: "열전달", course_code: "ME3021", term: "2026-2학기", computed_current_score: 88.4, computed_current_grade: "A0", html_url: "#" },
  { id: "102", name: "원자로이론", course_code: "NE3110", term: "2026-2학기", computed_current_score: 91.2, html_url: "#" },
  { id: "103", name: "기계시스템설계", course_code: "ME3105", term: "2026-2학기", computed_current_score: 79.5, html_url: "#" },
  { id: "104", name: "공학작문 및 발표", course_code: "GE2201", term: "2026-2학기", html_url: "#" },
];
const upcoming = {
  todo: [
    { context_name: "열전달", assignment: { id: "9001", name: "HW 6 — 핀 열전달 해석", due_at: iso(5), points_possible: 20, html_url: "#", submission: { workflow_state: "unsubmitted" } } },
    { context_name: "원자로이론", assignment: { id: "9002", name: "Home Work 8: 중성자 확산 방정식", due_at: iso(30), points_possible: 10, html_url: "#", submission: { workflow_state: "unsubmitted" } } },
  ],
  upcoming_events: [
    { context_name: "공학작문 및 발표", assignment: { id: "9003", name: "조사보고서 초안 제출", due_at: iso(70), points_possible: 30, html_url: "#", submission: { workflow_state: "submitted" } } },
    { context_name: "기계시스템설계", assignment: { id: "9004", name: "기어 설계 프로젝트 중간 발표 자료", due_at: iso(150), points_possible: 50, html_url: "#", submission: { workflow_state: "unsubmitted" } } },
  ],
};
const announcements = [
  { id: "a1", title: "11/4(화) 휴강 및 보강 안내", posted_at: iso(-20), author: "김교수", context_code: "course_101", message: "11월 4일 화요일 수업은 학회 참석으로 휴강합니다.\n보강은 11월 7일 금요일 15시, 기계관 301호에서 진행합니다." },
  { id: "a2", title: "중간고사 성적 공개 및 이의신청 기간", posted_at: iso(-50), author: "박교수", context_code: "course_102", message: "중간고사 성적이 공개되었습니다. 이의신청은 11/2까지 메일로 받습니다." },
];
const pending = [
  { id: 3, createdAt: Math.floor(now / 1000) - 120, kind: "planner_note", summary: "Canvas 할 일 추가 — 열전달 HW6 마무리 (2026-10-29T20:00:00+09:00)", reason: "마감 5시간 전이라 저녁에 마무리할 시간을 잡아두려고 합니다.", payload: { title: "열전달 HW6 마무리", todo_date: "2026-10-29T20:00:00+09:00", details: "핀 효율 계산 부분 검토" }, status: "pending", result: null, decidedAt: null },
  { id: 2, createdAt: Math.floor(now / 1000) - 600, kind: "submit_url", summary: "과제 URL 제출 — 공학작문 및 발표 / 조사보고서 초안 제출: https://docs.example.com/report", reason: "사용자가 보고서 링크 제출을 요청함", payload: { course_id: "104", assignment_id: "9003", url: "https://docs.example.com/report", _course_name: "공학작문 및 발표", _assignment_name: "조사보고서 초안 제출", _due_at: iso(70) }, status: "pending", result: null, decidedAt: null },
];
const jobs = [
  { id: 1, kind: "deadline_alert", enabled: true, intervalMin: 30, params: { hours_before: [24, 3] }, lastRun: Math.floor(now / 1000) - 900, lastResult: "미제출 과제 3개 확인, 알림 1개" },
  { id: 2, kind: "new_announcements", enabled: true, intervalMin: 30, params: {}, lastRun: Math.floor(now / 1000) - 900, lastResult: "새 공지 1개" },
  { id: 3, kind: "grade_updates", enabled: true, intervalMin: 60, params: {}, lastRun: Math.floor(now / 1000) - 3000, lastResult: "새 성적 0개" },
  { id: 4, kind: "ai_briefing", enabled: false, intervalMin: 1440, params: { time: "08:00", prompt: "이번 주 마감 과제와 최근 공지를 확인해서 오늘 해야 할 일을 우선순위대로 짧게 정리해줘." }, lastRun: null, lastResult: null },
];
const answer = `이번 주 마감 과제를 정리했어요.

| 마감 | 과목 | 과제 | 상태 |
|---|---|---|---|
| **오늘 23:59** (5시간 남음) | 열전달 | HW 6 — 핀 열전달 해석 | 미제출 |
| 내일 23:59 | 원자로이론 | Home Work 8 | 미제출 |
| 금 23:59 | 공학작문 및 발표 | 조사보고서 초안 | ✅ 제출함 |

**추천 순서**
1. 열전달 HW6부터 끝내세요. 마감이 가장 가깝고 배점(20점)도 큽니다.
2. 원자로이론 HW8은 내일 오전에 시작하면 충분합니다.

저녁 8시에 HW6 마무리 일정을 Canvas 할 일로 추가하자고 요청해 두었어요. **승인 대기**에서 승인하면 추가됩니다.`;
let threadsCalled = false;
mockIPC((cmd, args: any) => {
  switch (cmd) {
    case "app_info": return { version: "0.1.0", dataDir: "C:\\Users\\sopo\\AppData\\Roaming\\com.sopo9880.canvasassistant", hasToken: true, pending: 2,
      settings: { canvasBaseUrl: "https://canvas.example.ac.kr", closeToTray: true, notifications: true, codexPath: null, defaultModel: null, defaultEffort: null, canvasToolsInChat: true } };
    case "codex_start": return null;
    case "codex_locate": return { found: true, path: "C:\\Users\\sopo\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\...\\codex.exe" };
    case "codex_call": {
      const m = args.method;
      if (m === "account/read") return { account: loggedIn ? { type: "chatgpt", email: "student@example.com", planType: "plus" } : null };
      if (m === "model/list") return { data: [{ id: "gpt-6.1-sol", model: "gpt-6.1-sol", displayName: "GPT-6.1-Sol", isDefault: true, defaultReasoningEffort: "medium", supportedReasoningEfforts: [{ reasoningEffort: "low" }, { reasoningEffort: "medium" }, { reasoningEffort: "high" }] }] };
      if (m === "thread/list") { threadsCalled = true; return { data: [{ id: "t1", name: "이번 주 과제 정리", updatedAt: 1 }, { id: "t2", name: "원자로이론 HW8 개념 질문", updatedAt: 1 }, { id: "t3", preview: "열전달 핀 효율 공식이 왜 tanh가 나와?", updatedAt: 1 }] }; }
      if (m === "thread/resume") return {};
      if (m === "thread/turns/list") return { data: [{ items: [
        { type: "agentMessage", id: "i4", text: answer },
        { type: "mcpToolCall", id: "i3b", server: "canvas", tool: "propose_planner_note", status: "completed" },
        { type: "mcpToolCall", id: "i3", server: "canvas", tool: "canvas_upcoming", status: "completed" },
        { type: "mcpToolCall", id: "i2", server: "canvas", tool: "canvas_list_courses", status: "completed" },
        { type: "userMessage", id: "i1", content: [{ type: "text", text: "이번 주 마감인 과제를 마감 순서대로 정리해줘" }] },
      ].reverse() }] };
      return {};
    }
    case "canvas_query": {
      const t = args.tool;
      if (t === "canvas_list_courses") return courses;
      if (t === "canvas_upcoming") return upcoming;
      if (t === "canvas_list_announcements") return announcements;
      return [];
    }
    case "pending_list": return pending;
    case "jobs_list": return jobs;
    case "briefings_list": return [{ id: 1, at: Math.floor(now / 1000) - 3600 * 10, text: "**오늘 할 일**\n1. 열전달 HW6 (오늘 23:59)\n2. 원자로이론 HW8 준비\n\n새 공지: 열전달 11/4 휴강" }];
    case "plugin:autostart|is_enabled": return false;
    case "plugin:event|listen": return 1;
    case "plugin:event|unlisten": return null;
    default: return null;
  }
});
void threadsCalled;
import("../src/App").then(async ({ default: App }) => {
  const React = await import("react");
  const { createRoot } = await import("react-dom/client");
  await import("../src/styles.css");
  createRoot(document.getElementById("root")!).render(React.createElement(App));
  // Navigate to the requested view.
  const labels: Record<string, string> = { canvas: "Canvas", approvals: "승인 대기", automation: "자동화", settings: "설정" };
  setTimeout(() => {
    if (labels[view]) [...document.querySelectorAll<HTMLButtonElement>(".nav-item")].find((b) => b.textContent?.includes(labels[view]))?.click();
    if (view === "chat") setTimeout(() => document.querySelector<HTMLButtonElement>(".thread-title")?.click(), 300);
  }, 400);
});
