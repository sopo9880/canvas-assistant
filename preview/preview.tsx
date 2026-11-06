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
    case "codex_locate": return { found: true, source: "bundled", path: "C:\\Users\\sopo\\AppData\\Local\\Canvas Assistant\\codex\\codex.exe" };
    case "codex_call": {
      const m = args.method;
      if (m === "account/read") return { account: loggedIn ? { type: "chatgpt", email: "student@example.com", planType: "plus" } : null };
      if (m === "model/list") return { data: [{ id: "gpt-6.1-sol", model: "gpt-6.1-sol", displayName: "GPT-6.1-Sol", isDefault: true, defaultReasoningEffort: "medium", supportedReasoningEfforts: [{ reasoningEffort: "low" }, { reasoningEffort: "medium" }, { reasoningEffort: "high" }] }] };
      if (m === "thread/list") { threadsCalled = true; return { data: [{ id: "t1", name: "이번 주 과제 정리", updatedAt: 1 }, { id: "t2", name: "원자로이론 HW8 개념 질문", updatedAt: 1 }, { id: "t3", preview: "열전달 핀 효율 공식이 왜 tanh가 나와?", updatedAt: 1 }] }; }
      if (m === "thread/resume") return {};
      if (m === "thread/revert") return { thread: {} };
      if (m === "turn/steer") return { turnId: "u1" };
      if (m === "turn/start") return { turn: { id: "u9" } };
      if (m === "account/login/start") return args.params.type === "chatgpt" ? { type: "chatgpt", loginId: "l1", authUrl: "https://auth.openai.com/x" } : { type: "chatgptDeviceCode", loginId: "l2", userCode: "ABCD-1234", verificationUrl: "https://auth.openai.com/codex/device" };
      if (m === "thread/turns/list") return { data: [{ id: "u1", items: [
        { type: "agentMessage", id: "i6", text: "**이번_주_과제_정리.pdf**를 만들었습니다. 마감 순서표와 과제별 할 일이 들어 있어요." },
        { type: "mcpToolCall", id: "i5", server: "canvas", tool: "make_file", status: "completed", result: { content: [{ type: "text", text: JSON.stringify({ saved: true, name: "이번_주_과제_정리.pdf", path: "C:\\Users\\sopo\\Downloads\\Canvas Assistant\\AI 생성\\이번_주_과제_정리.pdf", size: 48213 }) }] } },
        { type: "agentMessage", id: "i4", text: answer },
        { type: "mcpToolCall", id: "i3b", server: "canvas", tool: "propose_planner_note", status: "completed" },
        { type: "mcpToolCall", id: "i3", server: "canvas", tool: "canvas_upcoming", status: "completed" },
        { type: "mcpToolCall", id: "i2", server: "canvas", tool: "canvas_list_courses", status: "completed" },
        { type: "userMessage", id: "i1", content: [{ type: "text", text: "이번 주 마감인 과제를 마감 순서대로 정리해줘\n\n[첨부 파일]\n- \"HW6_핀열전달.pdf\" (id=00000000000000000001, 470KB) — read_attachment로 내용 읽기" }, { type: "localImage", path: "C:\\x\\attachments\\00000000000000000002\\shot.png" }] },
      ].reverse() }] };
      return {};
    }
    case "chat_attach": return (args.paths as string[]).map((p: string, i: number) => ({ id: `0000000000000000000${i}`, name: p.split(/[\\/]/).pop(), size: 482133, image: /\.(png|jpe?g)$/i.test(p), path: p }));
    case "chat_attach_bytes": return { id: "000000000000000000aa", name: args.name, size: 2048, image: true, path: "x" };
    case "pick_files": return [{ path: "C:\\Users\\sopo\\Documents\\HW6_핀열전달.pdf", name: "HW6_핀열전달.pdf", size: 482133 }];
    case "canvas_action": return { ok: true, workflow_state: "submitted", attempt: 1 };
    case "canvas_query": {
      const t = args.tool;
      const ca = args.args ?? {};
      if (t === "canvas_course_tabs") return [
        { id: "home", label: "홈" }, { id: "syllabus", label: "수업 계획서" }, { id: "announcements", label: "공지" }, { id: "context_external_tool_2", label: "강의자료실", type: "external", html_url: "https://canvas.example.ac.kr/courses/101/external_tools/2" },
        { id: "discussions", label: "게시판" }, { id: "modules", label: "주차학습" }, { id: "assignments", label: "과제 및 평가" }, { id: "quizzes", label: "설문" },
        { id: "people", label: "사용자 및 그룹" }, { id: "context_external_tool_77", label: "출결현황", type: "external", html_url: "https://canvas.example.ac.kr/courses/101/external_tools/77" },
        { id: "grades", label: "성적" }, { id: "context_external_tool_88", label: "학습 분석", type: "external", html_url: "https://canvas.example.ac.kr/courses/101/external_tools/88" }];
      if (t === "canvas_course_home") return { name: "로봇AI 02분반", recent_activity: [{ type: "Announcement", title: "중간+기말고사 관련", message: "시험지에 분반을 기재하도록 되어 있습니다. 분반을 꼭 확인하세요.", updated_at: iso(-30), html_url: "#" }] };
      if (t === "canvas_list_assignments") return [upcoming.todo[0].assignment, { id: "9010", name: "Lab 3 보고서", due_at: iso(-50), points_possible: 15, submission_types: ["online_upload"], submission: { workflow_state: "graded", score: 13 } }, upcoming.upcoming_events[1].assignment];
      if (t === "canvas_get_assignment") return { ...upcoming.todo[0].assignment, submission_types: ["online_upload", "online_text_entry"], allowed_attempts: 2, allowed_extensions: ["pdf", "hwp"], description: "교재 3장 연습문제 3.12, 3.15를 풀고 핀 효율을 계산하세요.\n손글씨 스캔본(PDF) 또는 한글 파일로 제출합니다.",
        rubric: [{ description: "풀이 과정", points: 10, ratings: [{ description: "완전", points: 10 }, { description: "부분", points: 5 }] }, { description: "최종 답", points: 10, ratings: [{ description: "정답", points: 10 }, { description: "오답", points: 0 }] }] };
      if (t === "canvas_learningx_resources") return { count: 3, items: [
        { title: "1주차 강의 슬라이드", content_id: "a1", section: "1주차", type: "pdf", file_name: "week1.pdf", size: 2300000, created_at: iso(-300) },
        { title: "실습 예제 코드", content_id: "a2", section: "1주차", type: "zip", file_name: "lab1.zip", size: 18000, created_at: iso(-290) },
        { title: "2주차 강의 영상", content_id: "a3", section: "2주차", type: "movie", created_at: iso(-150) }] };
      if (t === "canvas_learningx_read") return { name: "week1.pdf", text: "열전달 1주차\n\n1. 전도, 대류, 복사\n2. 푸리에 법칙 q = -k dT/dx", total_chars: 40 };
      if (t === "canvas_read_submission") return { attempt: 2, attempts: [1, 2], submitted_at: iso(-20), files: [{ id: "x1", display_name: "main.cpp", size: 812, text: "#include <iostream>\nint main() {\n    std::cout << \"Hello\";\n}" }] };
      if (t === "canvas_my_submission") return { workflow_state: "submitted", history: [{ attempt: 1, submitted_at: iso(-40) }, { attempt: 2, submitted_at: iso(-20) }], attempt: 2, comments: [{ author_name: "김교수", comment: "단위를 꼭 표기하세요.", created_at: iso(-10) }] };
      if (t === "canvas_folders") return [{ id: "1", name: "course files", full_name: "course files", files_count: 0 }, { id: "2", name: "1주차", full_name: "course files/1주차", files_count: 3 }, { id: "3", name: "2주차", full_name: "course files/2주차", files_count: 2 }];
      if (t === "canvas_folder_files") return [{ id: "f1", display_name: "01_열전달_개요.pdf", size: 2400000, updated_at: iso(-200) }, { id: "f2", display_name: "01_연습문제.hwp", size: 120000, updated_at: iso(-190) }];
      if (t === "canvas_assignment_groups") return [{ id: "g1", name: "과제", group_weight: 30, assignments: [{ id: "9010", name: "Lab 3 보고서", points_possible: 15, submission: { id: "s1", score: 13 } }, { id: "9001", name: "HW 6", points_possible: 20, submission: { id: "s2" } }] }, { id: "g2", name: "중간고사", group_weight: 30, assignments: [{ id: "m1", name: "중간고사", points_possible: 100, submission: { id: "s3", score: 82 } }] }, { id: "g3", name: "기말고사", group_weight: 40, assignments: [{ id: "f1", name: "기말고사", points_possible: 100, submission: { id: "s4" } }] }];
      if (t === "canvas_conversations") return [{ id: "c1", subject: "HW6 제출 형식 문의", last_message: "PDF로 제출하면 됩니다.", last_message_at: iso(-5), participants: ["김교수", "나"], context_name: "열전달", workflow_state: "unread" }, { id: "c2", subject: "조 편성 안내", last_message: "3조로 배정되었습니다.", last_message_at: iso(-80), participants: ["박조교"], context_name: "기계시스템설계", workflow_state: "read" }];
      if (t === "canvas_conversation") return { subject: "HW6 제출 형식 문의", context_name: "열전달", participants: [{ id: 1, name: "김교수" }, { id: 2, name: "나" }], messages: [{ id: 2, author: "김교수", created_at: iso(-5), body: "PDF로 제출하면 됩니다." }, { id: 1, author: "나", created_at: iso(-6), body: "교수님, HW6을 한글 파일로 내도 될까요?" }] };
      if (t === "canvas_quizzes") return [{ id: "q1", title: "수업 만족도 설문", quiz_type: "survey", due_at: iso(100), allowed_attempts: 1 }];
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
