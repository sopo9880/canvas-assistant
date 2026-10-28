import { useEffect, useRef, useState } from "react";
import { api, errorText } from "./api";
import { useApp } from "./App";
import { open } from "./components";
import { canvasAction, CanvasRef, kindLabel } from "./shared";

export type MenuState = { x: number; y: number; ref: CanvasRef } | null;

const analyzePrompt: Partial<Record<CanvasRef["kind"], string>> = {
  assignment: "첨부한 과제를 분석해줘. 해야 할 일, 채점 기준(루브릭), 제출 방식과 허용 형식, 배점 비중, 마감까지 남은 시간을 정리하고 작업 순서를 제안해줘.",
  announcement: "첨부한 공지를 요약하고 내가 해야 할 일이 있으면 알려줘.",
  discussion: "첨부한 토론의 주제와 지금까지 나온 의견을 요약해줘.",
  file: "첨부한 파일이 어떤 자료인지 알려주고 관련 과제나 모듈이 있으면 찾아줘.",
  page: "첨부한 페이지 내용을 요약해줘.",
  quiz: "첨부한 퀴즈의 범위, 시간 제한, 응시 횟수와 마감을 정리하고 준비 방법을 알려줘.",
  course: "첨부한 강의의 남은 과제, 최근 공지, 현재 성적을 정리해줘.",
  module_item: "첨부한 모듈 항목을 확인하고 해야 할 일을 알려줘.",
  message: "첨부한 메시지를 요약하고 답장이 필요하면 초안을 써줘.",
  planner: "첨부한 할 일을 확인하고 해야 할 일을 정리해줘.",
};

const plannableType: Partial<Record<CanvasRef["kind"], string>> = {
  assignment: "assignment", quiz: "quiz", discussion: "discussion_topic", announcement: "announcement", page: "wiki_page",
};

export default function ContextMenu({ state, onClose }: { state: MenuState; onClose: () => void }) {
  const { askAI, addAttachment, openSubmit } = useApp();
  const ref = useRef<HTMLDivElement>(null);
  const [toast, setToast] = useState("");

  useEffect(() => {
    if (!state) return;
    const close = (e: Event) => { if (!ref.current?.contains(e.target as Node)) onClose(); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    window.addEventListener("blur", onClose);
    return () => { window.removeEventListener("mousedown", close); window.removeEventListener("keydown", esc); window.removeEventListener("blur", onClose); };
  }, [state, onClose]);

  useEffect(() => { if (toast) { const t = setTimeout(() => setToast(""), 2500); return () => clearTimeout(t); } }, [toast]);

  const r = state?.ref;
  const key = r ? `${r.kind}:${r.courseId}:${r.id}` : "";
  const run = (f: () => void | Promise<void>) => () => { onClose(); void f(); };
  const ptype = r && (plannableType[r.kind] ?? (r.kind === "planner" ? String(r.extra?.plannable_type ?? "") : ""));
  const pid = r?.kind === "planner" ? String(r.extra?.plannable_id ?? "") : r?.id;

  const x = state ? Math.min(state.x, window.innerWidth - 230) : 0;
  const y = state ? Math.min(state.y, window.innerHeight - 260) : 0;

  return (
    <>
      {state && r && (
        <div ref={ref} className="context-menu" style={{ left: x, top: y }} role="menu" aria-label={`${kindLabel[r.kind]} 메뉴`}>
          <div className="context-title">{kindLabel[r.kind]} · {r.label}</div>
          <button role="menuitem" onClick={run(() => { addAttachment({ type: "ref", key, ref: r }); setToast("AI 채팅에 추가했습니다"); })}>💬 AI 채팅에 추가</button>
          <button role="menuitem" onClick={run(() => askAI(analyzePrompt[r.kind] ?? "첨부한 항목을 분석해줘.", { send: true, refs: [r] }))}>✨ AI로 바로 분석</button>
          {r.kind === "assignment" && <button role="menuitem" onClick={run(() => openSubmit(r))}>📤 제출하기</button>}
          {r.kind === "file" && r.id && (
            <button role="menuitem" onClick={run(async () => { try { const p = await api.download(r.id!); setToast(`저장: ${p}`); } catch (e) { setToast(errorText(e)); } })}>⬇ 다운로드</button>
          )}
          {ptype && pid && (
            <>
              <button role="menuitem" onClick={run(async () => {
                try { await canvasAction("planner_complete", { plannable_type: ptype, plannable_id: pid, complete: true }); setToast("완료로 표시했습니다"); }
                catch (e) { setToast(errorText(e)); }
              })}>✓ 완료로 표시</button>
              <button role="menuitem" onClick={run(async () => {
                try { await canvasAction("planner_complete", { plannable_type: ptype, plannable_id: pid, complete: false }); setToast("완료 표시를 취소했습니다"); }
                catch (e) { setToast(errorText(e)); }
              })}>↺ 완료 취소</button>
            </>
          )}
          {r.htmlUrl && <button role="menuitem" onClick={run(() => open(r.htmlUrl))}>🌐 Canvas에서 열기</button>}
        </div>
      )}
      {toast && <div className="toast" role="status">{toast}</div>}
    </>
  );
}
