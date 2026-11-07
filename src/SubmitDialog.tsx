import { emit } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "./api";
import { Badge, Banner, open, Spinner } from "./components";
import { canvasAction, CanvasRef, Due, PickedFile, pickFiles } from "./shared";
import { assignmentStatus, bytes, formatDate, isSubmitted } from "./util";

type Mode = "online_upload" | "online_text_entry" | "online_url";
const modeLabel: Record<Mode, string> = { online_upload: "파일", online_text_entry: "텍스트", online_url: "웹 주소(URL)" };

/** Submit an assignment directly from the app (user action, confirmed here). */
export default function SubmitDialog({ target, onClose, onDone }: { target: CanvasRef; onClose: () => void; onDone?: () => void }) {
  const [a, setA] = useState<any>(null);
  const [sub, setSub] = useState<any>(null);
  const [error, setError] = useState("");
  const [mode, setMode] = useState<Mode | null>(null);
  const [files, setFiles] = useState<PickedFile[]>([]);
  const [text, setText] = useState("");
  const [url, setUrl] = useState("");
  const [comment, setComment] = useState("");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<any>(null);

  useEffect(() => {
    void (async () => {
      try {
        const [assignment, mine] = await Promise.all([
          api.canvas("canvas_get_assignment", { course_id: target.courseId, assignment_id: target.id }),
          api.canvas("canvas_my_submission", { course_id: target.courseId, assignment_id: target.id }).catch(() => null),
        ]);
        setA(assignment);
        setSub(mine);
        const types: string[] = assignment.submission_types ?? [];
        setMode((["online_upload", "online_text_entry", "online_url"] as Mode[]).find((m) => types.includes(m)) ?? null);
      } catch (e) { setError(errorText(e)); }
    })();
  }, [target]);

  const types: string[] = a?.submission_types ?? [];
  const modes = (["online_upload", "online_text_entry", "online_url"] as Mode[]).filter((m) => types.includes(m));
  const attempts = sub?.attempt ?? 0;
  const allowed = a?.allowed_attempts ?? -1;
  const attemptsLeft = allowed > 0 ? Math.max(0, allowed - attempts) : null;
  const exts: string[] | null = a?.allowed_extensions?.length ? a.allowed_extensions : null;
  const blocked = a?.locked_for_user ? (a?.lock_explanation || "이 과제는 잠겨 있습니다.") : attemptsLeft === 0 ? "남은 제출 횟수가 없습니다." : modes.length === 0 ? "이 과제는 앱에서 제출할 수 없는 방식입니다 (외부 도구, 오프라인, 미디어 등). Canvas에서 제출하세요." : "";

  const badExt = useMemo(() => exts ? files.filter((f) => !exts.includes((f.name.split(".").pop() ?? "").toLowerCase())) : [], [files, exts]);
  const ready = !blocked && !busy && (
    (mode === "online_upload" && files.length > 0 && badExt.length === 0) ||
    (mode === "online_text_entry" && text.trim().length > 0) ||
    (mode === "online_url" && /^https?:\/\/\S+$/i.test(url.trim()))
  );

  async function submit() {
    if (!ready || !mode) return;
    const what = mode === "online_upload" ? `파일 ${files.length}개` : mode === "online_text_entry" ? `텍스트 ${text.trim().length}자` : url.trim();
    const warn = attemptsLeft != null ? `\n남은 제출 횟수: ${attemptsLeft}회 → ${attemptsLeft - 1}회` : "";
    if (!window.confirm(`'${a.name}'에 ${what}을(를) 제출합니다.${warn}\n제출하면 되돌릴 수 없습니다. 계속할까요?`)) return;
    setBusy(true);
    setError("");
    try {
      const base = { course_id: target.courseId, assignment_id: target.id, comment: comment.trim() || undefined };
      const r = mode === "online_upload" ? await canvasAction("submit_files", { ...base, files: files.map((f) => f.path) })
        : mode === "online_text_entry" ? await canvasAction("submit_text", { ...base, body: text })
        : await canvasAction("submit_url", { ...base, url: url.trim() });
      setDone(r);
      onDone?.();
      // Dashboard and assignment lists reload, so the assignment stops showing as 미제출.
      void emit("canvas-updated", {}).catch(() => undefined);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  }

  return (
    <div className="modal" role="dialog" aria-modal aria-label="과제 제출" onClick={busy ? undefined : onClose}>
      <div className="modal-body wide" onClick={(e) => e.stopPropagation()}>
        <h2>📤 과제 제출 — {target.label}</h2>
        {!a && !error && <Spinner label="과제 정보를 불러오는 중…" />}
        {error && <Banner onClose={() => setError("")}>{error}</Banner>}
        {done ? (
          <>
            <Banner tone="ok">제출했습니다. {done.attempt ? `(${done.attempt}번째 제출)` : ""} {done.files ? `파일: ${done.files.join(", ")}` : ""}</Banner>
            <div className="row end">
              {a?.html_url && <button onClick={() => open(a.html_url)}>Canvas에서 확인</button>}
              <button className="primary" onClick={onClose}>닫기</button>
            </div>
          </>
        ) : a && (
          <>
            <div className="row wrap">
              <Due iso={a.due_at} />
              {a.points_possible != null && <Badge>{a.points_possible}점</Badge>}
              <Badge tone={sub && isSubmitted({ submission: sub }) ? "ok" : "neutral"}>{sub ? assignmentStatus({ submission: sub }) : "제출 기록 없음"}</Badge>
              <Badge tone={attemptsLeft === 0 ? "danger" : "neutral"}>{allowed > 0 ? `제출 ${attempts}/${allowed}회` : `제출 ${attempts}회 (제한 없음)`}</Badge>
              {exts && <Badge tone="warn">허용 형식: {exts.join(", ")}</Badge>}
            </div>
            {a.lock_at && <p className="muted small">제출 마감(잠김): {formatDate(a.lock_at)}</p>}
            {blocked ? <Banner tone="info">{blocked}</Banner> : (
              <>
                {modes.length > 1 && (
                  <div className="tabs inline" role="tablist">
                    {modes.map((m) => <button key={m} role="tab" aria-selected={mode === m} className={mode === m ? "active" : ""} onClick={() => setMode(m)}>{modeLabel[m]}</button>)}
                  </div>
                )}
                {mode === "online_upload" && (
                  <div className="submit-files">
                    <button onClick={async () => {
                      try {
                        const picked = await pickFiles(exts);
                        setFiles((old) => [...old, ...picked.filter((p) => !old.some((o) => o.path === p.path))]);
                      } catch (e) { setError(errorText(e)); }
                    }}>📎 파일 선택</button>
                    {files.length === 0 ? <p className="muted small">제출할 파일을 선택하세요. 여러 개를 고를 수 있습니다.</p> : (
                      <ul className="list compact">
                        {files.map((f) => (
                          <li key={f.path} className="list-item">
                            <span className="grow">{f.name} <span className="muted small">{bytes(f.size)}</span></span>
                            {badExt.includes(f) && <Badge tone="danger">허용되지 않는 형식</Badge>}
                            <button className="ghost icon" aria-label={`${f.name} 빼기`} onClick={() => setFiles((old) => old.filter((x) => x.path !== f.path))}>✕</button>
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
                {mode === "online_text_entry" && (
                  <label>제출할 내용<textarea rows={8} value={text} onChange={(e) => setText(e.target.value)} placeholder="직접 작성한 답안을 입력하세요" /></label>
                )}
                {mode === "online_url" && (
                  <label>제출할 주소<input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://..." /></label>
                )}
                <label>코멘트 (선택)<input value={comment} onChange={(e) => setComment(e.target.value)} placeholder="교수님께 남길 말" maxLength={2000} /></label>
              </>
            )}
            <div className="row end">
              <button onClick={onClose} disabled={busy}>취소</button>
              {!blocked && <button className="primary" disabled={!ready} onClick={() => void submit()}>{busy ? "제출 중…" : "제출"}</button>}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
