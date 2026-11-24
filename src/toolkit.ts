import type { Attachment } from "./App";
import { describeRef } from "./shared";

export type ParamType = "course" | "assignment" | "topic" | "folder" | "group" | "conversation" | "number" | "text" | "json" | "select" | "bool";
export type ToolParam = { key: string; label: string; type: ParamType; required?: boolean; options?: Array<[string, string]>; placeholder?: string; dependsOn?: string };
export type ToolDef = { name: string; label: string; category: string; desc: string; params: ToolParam[] };

const course: ToolParam = { key: "course_id", label: "강의", type: "course", required: true };
const assignment: ToolParam = { key: "assignment_id", label: "과제", type: "assignment", required: true, dependsOn: "course_id" };

/** Canvas tools the user can run from the chat composer (same tools the AI has). */
export const TOOLS: ToolDef[] = [
  { name: "canvas_upcoming", label: "다가오는 할 일·일정", category: "과제", desc: "모든 강의의 Canvas 할 일과 다가오는 마감", params: [] },
  { name: "canvas_planner", label: "플래너", category: "과제", desc: "날짜별 과제·퀴즈·토론·메모와 완료 상태", params: [{ key: "days_ahead", label: "앞으로 며칠", type: "number", placeholder: "21" }] },
  { name: "canvas_list_assignments", label: "과제 목록", category: "과제", desc: "강의의 과제와 내 제출 상태", params: [course, { key: "bucket", label: "필터", type: "select", options: [["", "전체"], ["upcoming", "다가오는"], ["unsubmitted", "미제출"], ["overdue", "기한 지남"], ["past", "지난 과제"]] }] },
  { name: "canvas_get_assignment", label: "과제 상세·루브릭", category: "과제", desc: "설명, 루브릭, 허용 형식, 제출 횟수", params: [course, assignment] },
  { name: "canvas_my_submission", label: "내 제출·피드백", category: "과제", desc: "제출 이력, 첨부, 교수 코멘트, 루브릭 평가", params: [course, assignment] },
  { name: "canvas_read_submission", label: "내 제출물 내용", category: "과제", desc: "내가 낸 글·코드·파일의 실제 내용", params: [course, assignment, { key: "attempt", label: "몇 번째 제출 (비우면 최신)", type: "number" }] },
  { name: "canvas_peer_reviews", label: "피어 리뷰", category: "과제", desc: "내가 해야 할 동료 평가", params: [course, assignment] },
  { name: "canvas_list_courses", label: "내 강의", category: "강의", desc: "수강 중인 강의와 현재 점수", params: [] },
  { name: "canvas_course_tabs", label: "강의 메뉴", category: "강의", desc: "학교가 설정한 강의 메뉴 구성", params: [course] },
  { name: "canvas_course_home", label: "강의 홈·최근 활동", category: "강의", desc: "첫 페이지와 최근 활동", params: [course] },
  { name: "canvas_syllabus", label: "수업 계획서", category: "강의", desc: "평가 방법, 일정, 규정", params: [course] },
  { name: "canvas_list_modules", label: "주차학습(모듈)", category: "강의", desc: "모듈과 항목", params: [course] },
  { name: "canvas_get_page", label: "페이지 읽기", category: "강의", desc: "강의 페이지 본문", params: [course, { key: "page_url", label: "페이지 주소(slug)", type: "text", required: true }] },
  { name: "canvas_quizzes", label: "퀴즈·설문", category: "강의", desc: "시간 제한, 응시 횟수, 마감", params: [course] },
  { name: "canvas_course_people", label: "강의 사용자", category: "강의", desc: "교수·조교·학생 목록", params: [course, { key: "search", label: "이름 검색", type: "text" }] },
  { name: "canvas_list_announcements", label: "공지", category: "공지·토론", desc: "최근 공지 (강의 선택 안 하면 전체)", params: [{ key: "course_ids", label: "강의", type: "course" }, { key: "days_back", label: "최근 며칠", type: "number", placeholder: "14" }] },
  { name: "canvas_list_discussions", label: "게시판 목록", category: "공지·토론", desc: "토론 주제", params: [course] },
  { name: "canvas_discussion_thread", label: "게시글 전체", category: "공지·토론", desc: "본문과 모든 댓글", params: [course, { key: "topic_id", label: "게시글", type: "topic", required: true, dependsOn: "course_id" }] },
  { name: "canvas_folders", label: "강의자료 폴더", category: "자료", desc: "강의자료실 폴더 구조", params: [course] },
  { name: "canvas_folder_files", label: "폴더의 파일", category: "자료", desc: "폴더 안 파일 목록", params: [course, { key: "folder_id", label: "폴더", type: "folder", required: true, dependsOn: "course_id" }] },
  { name: "canvas_list_files", label: "파일 검색", category: "자료", desc: "강의 파일 (최신순)", params: [course, { key: "search", label: "검색어", type: "text" }] },
  { name: "canvas_read_file", label: "파일 내용 읽기", category: "자료", desc: "PDF·PPT·Word·한글·코드 파일의 텍스트", params: [{ key: "file_id", label: "파일 ID", type: "number", required: true }] },
  { name: "canvas_learningx_resources", label: "LearningX 강의자료실", category: "자료", desc: "LearningX로 운영되는 강의자료실 목록", params: [course] },
  { name: "canvas_learningx_weekly", label: "LearningX 주차학습", category: "자료", desc: "주차별 강의·기한·출결·할 일과 주차 안의 과제 (읽기만, 시청 처리 안 됨)", params: [course] },
  { name: "canvas_learningx_read", label: "LearningX 자료 읽기", category: "자료", desc: "LearningX 자료의 텍스트 (content_id)", params: [course, { key: "content_id", label: "content_id", type: "text", required: true }] },
  { name: "canvas_grades", label: "전체 성적", category: "성적", desc: "강의별 현재·최종 점수", params: [] },
  { name: "canvas_assignment_groups", label: "평가 비중·과제 점수", category: "성적", desc: "평가 항목 비율과 과제별 점수", params: [course] },
  { name: "canvas_list_submissions", label: "제출 현황", category: "성적", desc: "강의의 내 제출과 점수", params: [course] },
  { name: "canvas_calendar", label: "캘린더", category: "일정", desc: "다가오는 일정과 마감", params: [{ key: "days_ahead", label: "앞으로 며칠", type: "number", placeholder: "14" }] },
  { name: "canvas_conversations", label: "메시지함", category: "메시지·그룹", desc: "Canvas 메시지 목록", params: [{ key: "scope", label: "보기", type: "select", options: [["inbox", "받은 메시지"], ["unread", "안 읽음"], ["sent", "보낸 메시지"]] }] },
  { name: "canvas_conversation", label: "메시지 읽기", category: "메시지·그룹", desc: "대화의 모든 메시지", params: [{ key: "conversation_id", label: "대화", type: "conversation", required: true }] },
  { name: "canvas_groups", label: "내 그룹", category: "메시지·그룹", desc: "조별 과제 그룹", params: [] },
  { name: "canvas_group_members", label: "그룹 구성원", category: "메시지·그룹", desc: "그룹 멤버", params: [{ key: "group_id", label: "그룹", type: "group", required: true }] },
  { name: "canvas_profile", label: "내 프로필", category: "기타", desc: "이름, 시간대", params: [] },
  { name: "canvas_api_get", label: "Canvas API 직접 조회", category: "기타", desc: "읽기 전용. 예: courses/123/pages", params: [{ key: "path", label: "경로 (/api/v1 뒤)", type: "text", required: true, placeholder: "users/self/activity_stream" }, { key: "query", label: "쿼리 (JSON)", type: "json", placeholder: "{\"per_page\": 20}" }] },
];

export const CATEGORIES = Array.from(new Set(TOOLS.map((t) => t.category)));

const RESULT_LIMIT = 12000;

/** The text actually sent to Codex: the user's words plus attached Canvas context. */
export function composeMessage(text: string, atts: Attachment[]): string {
  if (!atts.length) return text;
  const refs = atts.filter((a): a is Extract<Attachment, { type: "ref" }> => a.type === "ref");
  const files = atts.filter((a): a is Extract<Attachment, { type: "file" }> => a.type === "file").map((a) => a.file);
  const results = atts.filter((a): a is Extract<Attachment, { type: "tool" }> => a.type === "tool" && a.mode === "result");
  const instruct = atts.filter((a): a is Extract<Attachment, { type: "tool" }> => a.type === "tool" && a.mode === "instruct");
  const out = [text || (files.length && files.length === atts.length ? "첨부한 파일을 확인해줘." : "첨부한 항목을 확인해줘.")];
  if (files.length) {
    out.push("[첨부 파일]\n" + files.map((f) => f.image
      ? `- 이미지 "${f.name}" (메시지에 이미지로 포함)`
      : `- "${f.name}" (id=${f.id}, ${Math.max(1, Math.round(f.size / 1024))}KB) — read_attachment로 내용 읽기`).join("\n"));
  }
  if (refs.length) out.push("[첨부된 Canvas 항목]\n" + refs.map((r) => `- ${describeRef(r.ref)}`).join("\n"));
  for (const r of results) {
    let json = JSON.stringify(r.result ?? null);
    if (json.length > RESULT_LIMIT) json = json.slice(0, RESULT_LIMIT) + "…(생략)";
    out.push(`[도구 결과: ${r.tool} ${JSON.stringify(r.args)}]\n\`\`\`json\n${json}\n\`\`\``);
  }
  if (instruct.length) out.push("[도구 사용 지시]\n" + instruct.map((t) => `- ${t.tool}(${JSON.stringify(t.args)}) 도구를 사용해서 답해줘.`).join("\n"));
  return out.join("\n\n");
}

/** Splits a stored user message into what the user typed and the attached context. */
export function splitMessage(text: string): { main: string; context: string | null } {
  const i = text.search(/\n\n\[(첨부 파일|첨부된 Canvas 항목|도구 결과: |도구 사용 지시)\]?/);
  return i === -1 ? { main: text, context: null } : { main: text.slice(0, i), context: text.slice(i + 2) };
}
