import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorText, Job } from "../api";
import { useApp } from "../App";
import { Banner, Empty, Markdown, Spinner, Toggle } from "../components";
import { formatUnix } from "../util";

const meta: Record<Job["kind"], { title: string; desc: string; icon: string }> = {
  deadline_alert: { title: "마감 알림", desc: "제출하지 않은 과제의 마감이 가까워지면 Windows 알림을 보냅니다.", icon: "⏰" },
  new_announcements: { title: "새 공지 알림", desc: "수강 중인 강의에 새 공지가 올라오면 알려줍니다.", icon: "📢" },
  grade_updates: { title: "성적 등록 알림", desc: "과제 점수가 새로 매겨지면 알려줍니다.", icon: "📝" },
  ai_briefing: { title: "AI 데일리 브리핑", desc: "매일 정한 시각에 Codex가 Canvas를 확인해 오늘 할 일을 정리합니다. ChatGPT 사용량이 소모됩니다.", icon: "☀️" },
};

function JobCard({ job, onSaved, canvasReady }: { job: Job; onSaved: () => void; canvasReady: boolean }) {
  const [enabled, setEnabled] = useState(job.enabled);
  const [interval, setIntervalMin] = useState(job.intervalMin);
  const [hours, setHours] = useState<string>((job.params.hours_before ?? [24, 3]).join(", "));
  const [time, setTime] = useState<string>(job.params.time ?? "08:00");
  const [prompt, setPrompt] = useState<string>(job.params.prompt ?? "");
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const m = meta[job.kind];

  async function save(next = enabled) {
    setError("");
    let params: Record<string, unknown> = job.params;
    if (job.kind === "deadline_alert") {
      const list = hours.split(/[,\s]+/).map((x) => parseInt(x, 10)).filter((n) => n >= 1 && n <= 168);
      if (list.length === 0) { setError("알림 시점을 1~168 사이 시간으로 입력하세요. 예: 24, 3"); return; }
      params = { hours_before: Array.from(new Set(list)).sort((a, b) => b - a) };
    }
    if (job.kind === "ai_briefing") params = { time, prompt };
    try {
      await api.updateJob(job.kind, next, interval, params);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
      onSaved();
    } catch (e) { setError(errorText(e)); }
  }

  return (
    <section className="card job">
      <div className="row space">
        <h2><span aria-hidden>{m.icon}</span> {m.title}</h2>
        <Toggle label="" checked={enabled} onChange={(v) => { setEnabled(v); void save(v); }} />
      </div>
      <p className="muted">{m.desc}</p>
      {job.kind !== "ai_briefing" ? (
        <div className="form-grid">
          <label>확인 주기
            <select value={interval} onChange={(e) => setIntervalMin(Number(e.target.value))}>
              {[15, 30, 60, 120, 360].map((n) => <option key={n} value={n}>{n < 60 ? `${n}분` : `${n / 60}시간`}마다</option>)}
            </select>
          </label>
          {job.kind === "deadline_alert" && (
            <label>마감 몇 시간 전에 알릴까요?
              <input value={hours} onChange={(e) => setHours(e.target.value)} placeholder="24, 3" />
            </label>
          )}
        </div>
      ) : (
        <div className="form-grid">
          <label>실행 시각<input type="time" value={time} onChange={(e) => setTime(e.target.value)} /></label>
          <label className="span-2">AI에게 할 요청
            <textarea rows={3} value={prompt} maxLength={2000} onChange={(e) => setPrompt(e.target.value)} />
          </label>
        </div>
      )}
      {error && <Banner onClose={() => setError("")}>{error}</Banner>}
      <div className="row space">
        <span className="muted small">
          {job.lastRun ? `마지막 실행 ${formatUnix(job.lastRun)} · ${job.lastResult ?? ""}` : "아직 실행되지 않았습니다"}
          {!canvasReady && job.kind !== "ai_briefing" && " · Canvas 연결 필요"}
        </span>
        <div className="row">
          <button onClick={() => void api.runJob(job.kind).then(onSaved).catch((e) => setError(errorText(e)))}>지금 실행</button>
          <button className="primary" onClick={() => void save()}>{saved ? "저장됨 ✓" : "저장"}</button>
        </div>
      </div>
    </section>
  );
}

export default function Automation() {
  const { info } = useApp();
  const [jobs, setJobs] = useState<Job[] | null>(null);
  const [briefings, setBriefings] = useState<Array<{ id: number; at: number; text: string }>>([]);
  const [error, setError] = useState("");
  const canvasReady = !!(info?.hasToken && info.settings.canvasBaseUrl);

  const load = useCallback(async () => {
    try {
      setJobs(await api.jobs());
      setBriefings(await api.briefings());
    } catch (e) { setError(errorText(e)); }
  }, []);

  useEffect(() => {
    void load();
    const subs = [listen("jobs-updated", () => void load()), listen("briefing-updated", () => void load())];
    return () => { subs.forEach((p) => void p.then((f) => f())); };
  }, [load]);

  return (
    <div className="page">
      <header className="topbar"><h1>자동화</h1></header>
      <div className="content narrow">
        <p className="muted">앱이 열려 있거나 트레이에서 실행 중일 때 동작합니다. 설정에서 'Windows 시작 시 자동 실행'을 켜면 항상 알림을 받을 수 있습니다.</p>
        {info && !info.settings.notifications && <Banner tone="info">Windows 알림이 꺼져 있습니다. 설정에서 켜야 알림이 표시됩니다.</Banner>}
        {error && <Banner onClose={() => setError("")}>{error}</Banner>}
        {jobs === null ? <Spinner label="불러오는 중…" /> : jobs.map((j) => <JobCard key={`${j.kind}-${j.lastRun}`} job={j} onSaved={() => void load()} canvasReady={canvasReady} />)}
        <section className="card">
          <h2>지난 브리핑</h2>
          {briefings.length === 0 ? <Empty title="아직 브리핑이 없습니다" icon="☀️">AI 데일리 브리핑을 켜거나 '지금 실행'을 눌러보세요.</Empty> : (
            briefings.map((b) => (
              <details key={b.id} className="briefing" open={b.id === briefings[0].id}>
                <summary>{formatUnix(b.at)}</summary>
                <Markdown text={b.text} />
              </details>
            ))
          )}
        </section>
      </div>
    </div>
  );
}
