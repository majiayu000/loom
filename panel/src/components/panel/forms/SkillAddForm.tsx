import { useState, type CSSProperties, type FormEvent } from "react";
import { api } from "../../../lib/api/client";
import { useMutation } from "../../../lib/useMutation";

export function SkillAddForm({ onCancel, onSuccess }: { onCancel: () => void; onSuccess: () => void }) {
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const add = useMutation();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmedSource = source.trim();
    const trimmedName = name.trim();
    if (!trimmedSource || !trimmedName) return;
    add.run("skill add", () => api.skillAdd({ source: trimmedSource, name: trimmedName }), onSuccess);
  };

  return (
    <form onSubmit={submit} className="card" style={{ padding: 16, marginBottom: 12 }}>
      <div style={{ display: "grid", gridTemplateColumns: "120px 1fr", gap: 8, alignItems: "center" }}>
        <label className="hint">source</label>
        <input
          value={source}
          onChange={(event) => setSource(event.target.value)}
          placeholder="/Users/me/.claude/skills/my-skill"
          style={formInputStyle}
          autoFocus
        />
        <label className="hint">name</label>
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="my-skill"
          style={formInputStyle}
        />
      </div>
      {(add.error || add.success) && <div style={add.error ? errorStyle : okStyle}>{add.error ?? `✓ ${add.success}`}</div>}
      <div style={{ display: "flex", gap: 8, marginTop: 12, justifyContent: "flex-end" }}>
        <button type="button" className="btn ghost" onClick={onCancel} disabled={add.busy}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={add.busy || !source.trim() || !name.trim()}>
          {add.busy ? "adding…" : "skill add"}
        </button>
      </div>
    </form>
  );
}

const formInputStyle: CSSProperties = {
  padding: "6px 10px",
  borderRadius: 6,
  border: "1px solid var(--line-hi)",
  background: "var(--bg-2)",
  color: "var(--ink-0)",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  minWidth: 0,
};

const errorStyle: CSSProperties = {
  marginTop: 10,
  padding: "6px 10px",
  color: "var(--err)",
  background: "rgba(216,90,90,0.08)",
  border: "1px solid rgba(216,90,90,0.3)",
  borderRadius: 6,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
};

const okStyle: CSSProperties = {
  ...errorStyle,
  color: "var(--ok)",
  background: "rgba(111,183,138,0.08)",
  border: "1px solid rgba(111,183,138,0.3)",
};
