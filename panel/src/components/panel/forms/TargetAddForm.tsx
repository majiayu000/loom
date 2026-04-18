import { useState } from "react";
import { AGENT_OPTIONS } from "../../../lib/agent_options";
import { api } from "../../../lib/api/client";

type Ownership = "managed" | "observed" | "external";
const OWNERSHIPS: Ownership[] = ["managed", "observed", "external"];

interface TargetAddFormProps {
  onCancel: () => void;
  onSuccess: () => void;
}

export function TargetAddForm({ onCancel, onSuccess }: TargetAddFormProps) {
  const [agent, setAgent] = useState<string>(AGENT_OPTIONS[0].slug);
  const [path, setPath] = useState("");
  const [ownership, setOwnership] = useState<Ownership>("managed");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!path.trim()) {
      setError("path required");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.targetAdd({ agent, path: path.trim(), ownership });
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="card" style={{ padding: 16, marginBottom: 12 }}>
      <div style={{ display: "grid", gridTemplateColumns: "120px 1fr", gap: 8, alignItems: "center" }}>
        <label className="hint">agent</label>
        <select value={agent} onChange={(e) => setAgent(e.target.value)} style={inputStyle}>
          {AGENT_OPTIONS.map((a) => (
            <option key={a.slug} value={a.slug}>
              {a.label}
            </option>
          ))}
        </select>
        <label className="hint">path</label>
        <input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="$HOME/.claude/skills"
          style={inputStyle}
          autoFocus
        />
        <label className="hint">ownership</label>
        <select value={ownership} onChange={(e) => setOwnership(e.target.value as Ownership)} style={inputStyle}>
          {OWNERSHIPS.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      </div>
      {error && <div style={errorStyle}>{error}</div>}
      <div style={{ display: "flex", gap: 8, marginTop: 12, justifyContent: "flex-end" }}>
        <button type="button" className="btn ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={busy}>
          {busy ? "adding…" : "target add"}
        </button>
      </div>
    </form>
  );
}

const inputStyle: React.CSSProperties = {
  padding: "6px 10px",
  borderRadius: 6,
  border: "1px solid var(--line-hi)",
  background: "var(--bg-2)",
  color: "var(--ink-0)",
  fontSize: 12.5,
  fontFamily: "var(--font-mono)",
};

const errorStyle: React.CSSProperties = {
  marginTop: 10,
  padding: "6px 10px",
  color: "var(--err)",
  background: "rgba(216,90,90,0.08)",
  border: "1px solid rgba(216,90,90,0.3)",
  borderRadius: 6,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
};
