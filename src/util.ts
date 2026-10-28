const weekdays = ["일", "월", "화", "수", "목", "금", "토"];

export function formatDate(iso?: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${d.getMonth() + 1}/${d.getDate()}(${weekdays[d.getDay()]}) ${hh}:${mm}`;
}

export function formatUnix(sec?: number | null): string {
  if (!sec) return "";
  return formatDate(new Date(sec * 1000).toISOString());
}

/** "3시간 남음", "2일 남음", "1일 지남" */
export function timeLeft(iso?: string | null, now = Date.now()): { label: string; tone: "danger" | "warn" | "ok" | "past" } | null {
  if (!iso) return null;
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return null;
  const diff = t - now;
  const abs = Math.abs(diff);
  const h = abs / 3_600_000;
  const amount = h < 1 ? `${Math.max(1, Math.round(abs / 60_000))}분` : h < 48 ? `${Math.round(h)}시간` : `${Math.round(h / 24)}일`;
  if (diff < 0) return { label: `${amount} 지남`, tone: "past" };
  return { label: `${amount} 남음`, tone: h <= 24 ? "danger" : h <= 72 ? "warn" : "ok" };
}

export function submissionLabel(state?: string, opts: { missing?: boolean; late?: boolean; excused?: boolean } = {}): string {
  if (opts.excused) return "면제";
  switch (state) {
    case "graded": return "채점됨";
    case "submitted": return opts.late ? "늦게 제출" : "제출함";
    case "pending_review": return "검토 중";
    case "unsubmitted": return opts.missing ? "미제출(누락)" : "미제출";
    default: return state ?? "";
  }
}

export function isSubmitted(a: any): boolean {
  const s = a?.submission?.workflow_state;
  return s === "submitted" || s === "graded" || s === "pending_review" || a?.submission?.excused === true;
}

export function bytes(n?: number): string {
  if (!n && n !== 0) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(1)} GB`;
}

export const toolLabels: Record<string, string> = {
  canvas_profile: "프로필 확인",
  canvas_list_courses: "강의 목록 조회",
  canvas_upcoming: "다가오는 일정 조회",
  canvas_list_assignments: "과제 목록 조회",
  canvas_get_assignment: "과제 내용 확인",
  canvas_list_announcements: "공지 조회",
  canvas_grades: "성적 조회",
  canvas_list_submissions: "제출 현황 조회",
  canvas_list_files: "파일 목록 조회",
  canvas_list_modules: "모듈 조회",
  canvas_get_page: "페이지 읽기",
  canvas_list_discussions: "토론 조회",
  canvas_calendar: "캘린더 조회",
  propose_submit_text: "과제 제출 요청 (승인 필요)",
  propose_submit_url: "URL 제출 요청 (승인 필요)",
  propose_discussion_reply: "토론 댓글 요청 (승인 필요)",
  propose_planner_note: "할 일 추가 요청 (승인 필요)",
  propose_calendar_event: "일정 추가 요청 (승인 필요)",
};

export const effortLabels: Record<string, string> = {
  none: "없음", minimal: "최소", low: "낮음", medium: "보통", high: "높음", xhigh: "매우 높음", max: "최대", ultra: "울트라",
};
