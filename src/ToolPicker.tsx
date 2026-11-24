import { useEffect, useMemo, useState } from "react";
import { api, errorText } from "./api";
import { useApp } from "./App";
import { Banner, Spinner } from "./components";
import { CATEGORIES, ToolDef, ToolParam, TOOLS } from "./toolkit";

type Opt = { value: string; label: string };
const cache = new Map<string, Promise<Opt[]>>();

function loadOptions(type: ToolParam["type"], parent?: string): Promise<Opt[]> {
  const key = `${type}:${parent ?? ""}`;
  if (!cache.has(key)) {
    const p = (async (): Promise<Opt[]> => {
      switch (type) {
        case "course": return (await api.canvas<any[]>("canvas_list_courses")).map((c) => ({ value: String(c.id), label: c.name }));
        case "assignment": return parent ? (await api.canvas<any[]>("canvas_list_assignments", { course_id: parent })).map((a) => ({ value: String(a.id), label: a.name })) : [];
        case "topic": return parent ? (await api.canvas<any[]>("canvas_list_discussions", { course_id: parent })).map((t) => ({ value: String(t.id), label: t.title })) : [];
        case "folder": return parent ? (await api.canvas<any[]>("canvas_folders", { course_id: parent })).map((f) => ({ value: String(f.id), label: f.full_name })) : [];
        case "group": return (await api.canvas<any[]>("canvas_groups")).map((g) => ({ value: String(g.id), label: g.name }));
        case "conversation": return (await api.canvas<any[]>("canvas_conversations", { scope: "inbox" })).map((c) => ({ value: String(c.id), label: c.subject || "(제목 없음)" }));
        default: return [];
      }
    })();
    p.catch(() => cache.delete(key));
    cache.set(key, p);
  }
  return cache.get(key)!;
}

function ParamInput({ p, value, parent, onChange }: { p: ToolParam; value: string; parent?: string; onChange: (v: string, label?: string) => void }) {
  const [opts, setOpts] = useState<Opt[] | null>(null);
  const [err, setErr] = useState("");
  const remote = ["course", "assignment", "topic", "folder", "group", "conversation"].includes(p.type);
  useEffect(() => {
    if (!remote) return;
    if (p.dependsOn && !parent) { setOpts([]); return; }
    setOpts(null);
    loadOptions(p.type, parent).then(setOpts).catch((e) => setErr(errorText(e)));
  }, [p.type, parent, remote, p.dependsOn]);
  const label = <span>{p.label}{p.required && " *"}</span>;
  if (remote) {
    return (
      <label>{label}
        {err ? <span className="error-text small">{err}</span> : opts === null ? <Spinner /> : (
          <select value={value} onChange={(e) => onChange(e.target.value, opts.find((o) => o.value === e.target.value)?.label)} disabled={!!p.dependsOn && !parent}>
            <option value="">{p.dependsOn && !parent ? "먼저 강의를 고르세요" : p.required ? "선택하세요" : "(전체)"}</option>
            {opts.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </select>
        )}
      </label>
    );
  }
  if (p.type === "select") return <label>{label}<select value={value} onChange={(e) => onChange(e.target.value)}>{p.options!.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select></label>;
  if (p.type === "json") return <label>{label}<textarea rows={2} value={value} onChange={(e) => onChange(e.target.value)} placeholder={p.placeholder} /></label>;
  return <label>{label}<input value={value} inputMode={p.type === "number" ? "numeric" : undefined} onChange={(e) => onChange(p.type === "number" ? e.target.value.replace(/[^0-9]/g, "") : e.target.value)} placeholder={p.placeholder} /></label>;
}

export default function ToolPicker({ onClose }: { onClose: () => void }) {
  const { addAttachment } = useApp();
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState<string>("전체");
  const [tool, setTool] = useState<ToolDef | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<string | null>(null);

  const shown = useMemo(() => TOOLS.filter((t) => (cat === "전체" || t.category === cat) && (!query || (t.label + t.desc + t.name).toLowerCase().includes(query.toLowerCase()))), [cat, query]);
  const missing = tool?.params.some((p) => p.required && !values[p.key]);

  const args = (): Record<string, unknown> => {
    const out: Record<string, unknown> = {};
    for (const p of tool?.params ?? []) {
      const v = values[p.key];
      if (!v) continue;
      if (p.type === "number") out[p.key] = Number(v);
      else if (p.type === "json") out[p.key] = JSON.parse(v);
      else if (p.key === "course_ids") out[p.key] = [v];
      else out[p.key] = v;
    }
    return out;
  };
  const chipLabel = () => {
    const extra = Object.entries(labels).filter(([k]) => values[k]).map(([, l]) => l).join(" · ");
    return `${tool!.label}${extra ? ` — ${extra}` : ""}`;
  };

  async function attachResult() {
    if (!tool) return;
    setBusy(true); setError(""); setPreview(null);
    try {
      const a = args();
      const result = await api.canvas(tool.name, a);
      addAttachment({ type: "tool", key: `tool:${tool.name}:${JSON.stringify(a)}`, tool: tool.name, label: chipLabel(), args: a, mode: "result", result });
      const text = JSON.stringify(result, null, 1);
      setPreview(text.length > 1500 ? text.slice(0, 1500) + "\n…" : text);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  }

  function instruct() {
    if (!tool) return;
    try {
      const a = args();
      addAttachment({ type: "tool", key: `instruct:${tool.name}:${JSON.stringify(a)}`, tool: tool.name, label: `AI 지시: ${chipLabel()}`, args: a, mode: "instruct" });
      onClose();
    } catch (e) { setError(errorText(e)); }
  }

  return (
    <div className="modal" role="dialog" aria-modal aria-label="Canvas 도구" onClick={onClose}>
      <div className="modal-body wide tool-picker" onClick={(e) => e.stopPropagation()}>
        <div className="row space"><h2>🧰 Canvas 도구</h2><button className="ghost" onClick={onClose} aria-label="닫기">✕</button></div>
        <div className="picker-layout">
          <div className="picker-list">
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="도구 검색" aria-label="도구 검색" autoFocus />
            <div className="chips">{["전체", ...CATEGORIES].map((c) => <button key={c} className={"chip" + (cat === c ? " on" : "")} onClick={() => setCat(c)}>{c}</button>)}</div>
            <div className="picker-tools">
              {shown.map((t) => (
                <button key={t.name} className={"picker-tool" + (tool?.name === t.name ? " active" : "")} onClick={() => { setTool(t); setValues({}); setLabels({}); setPreview(null); setError(""); }}>
                  <strong>{t.label}</strong><span className="muted small">{t.desc}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="picker-form">
            {!tool ? <p className="muted">왼쪽에서 도구를 고르세요. 결과를 대화에 첨부하거나, AI에게 그 도구로 답하라고 지시할 수 있습니다.</p> : (
              <>
                <h3>{tool.label}</h3>
                <p className="muted small">{tool.desc} · <code>{tool.name}</code></p>
                {tool.params.length === 0 && <p className="muted small">입력할 값이 없습니다.</p>}
                {tool.params.map((p) => (
                  <ParamInput key={p.key} p={p} value={values[p.key] ?? ""} parent={p.dependsOn ? values[p.dependsOn] : undefined}
                    onChange={(v, l) => {
                      setValues((o) => {
                        const next = { ...o, [p.key]: v };
                        for (const q of tool.params) if (q.dependsOn === p.key) delete next[q.key];
                        return next;
                      });
                      setLabels((o) => ({ ...o, [p.key]: l ?? v }));
                    }} />
                ))}
                {error && <Banner onClose={() => setError("")}>{error}</Banner>}
                <div className="row wrap">
                  <button className="primary" disabled={busy || missing} onClick={() => void attachResult()}>{busy ? "실행 중…" : "실행해서 첨부"}</button>
                  <button disabled={busy || missing} onClick={instruct}>AI에게 이 도구로 답하게 하기</button>
                </div>
                {preview && (
                  <>
                    <Banner tone="ok">대화에 첨부했습니다. 메시지를 쓰고 보내면 AI가 이 결과를 참고합니다.</Banner>
                    <pre className="box small">{preview}</pre>
                  </>
                )}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
