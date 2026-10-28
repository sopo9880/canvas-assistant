import React from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";

export function open(url?: string | null) {
  if (url && /^https?:\/\//i.test(url)) void openUrl(url);
}

export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a href={href} onClick={(e) => { e.preventDefault(); open(href); }} title={href}>{children}</a>
          ),
          img: ({ alt }) => <span className="muted">[이미지: {alt || "첨부"}]</span>,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

export function Toggle({ checked, onChange, label, hint, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string; disabled?: boolean }) {
  return (
    <label className={"toggle" + (disabled ? " disabled" : "")}>
      <span className="toggle-text"><span>{label}</span>{hint && <small>{hint}</small>}</span>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className="switch" aria-hidden />
    </label>
  );
}

export function Banner({ tone = "error", children, onClose }: { tone?: "error" | "info" | "ok"; children: React.ReactNode; onClose?: () => void }) {
  return (
    <div role={tone === "error" ? "alert" : "status"} className={"banner " + tone}>
      <div>{children}</div>
      {onClose && <button className="ghost" onClick={onClose} aria-label="닫기">✕</button>}
    </div>
  );
}

export function Empty({ title, children, icon = "○" }: { title: string; children?: React.ReactNode; icon?: string }) {
  return (
    <div className="empty">
      <div className="empty-icon" aria-hidden>{icon}</div>
      <strong>{title}</strong>
      {children && <div className="muted">{children}</div>}
    </div>
  );
}

export function Spinner({ label }: { label?: string }) {
  return <span className="spinner-wrap"><span className="spinner" aria-hidden />{label && <span>{label}</span>}</span>;
}

export function Badge({ tone = "neutral", children }: { tone?: "neutral" | "danger" | "warn" | "ok" | "past" | "accent"; children: React.ReactNode }) {
  return <span className={"badge " + tone}>{children}</span>;
}
