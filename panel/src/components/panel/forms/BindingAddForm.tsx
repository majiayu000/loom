import { useState } from "react";
import { AGENT_OPTIONS } from "../../../lib/agent_options";
import type { Target } from "../../../lib/types";
import { api } from "../../../lib/api/client";

type MatcherKind = "path-prefix" | "exact-path" | "name";
const MATCHERS: MatcherKind[] = ["path-prefix", "exact-path", "name"];

interface BindingAddFormProps {
  targets: Target[];
  onCancel: () => void;
  onSuccess: () => void;
}

export function BindingAddForm({ targets, onCancel, onSuccess }: BindingAddFormProps) {
  const [agent, setAgent] = useState<string>(AGENT_OPTIONS[0].slug);
  const [profile, setProfile] = useState("home");
  const [matcherKind, setMatcherKind] = useState<MatcherKind>("path-prefix");
  const [matcherValue, setMatcherValue] = useState("");
  const [targetId, setTargetId] = useState(targets[0]?.id ?? "");
  const [policyProfile, setPolicyProfile] = useState("safe-capture");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!matcherValue.trim() || !targetId) {
      setError("matcher value + target required");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.bindingAdd({
        agent,
        profile: profile.trim() || "home",
        matcher_kind: matcherKind,
        matcher_value: matcherValue.trim(),
        target: targetId,
        policy_profile: policyProfile.trim() || undefined,
      });
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
        <label className="hint">profile</label>
        <input value={profile} onChange={(e) => setProfile(e.target.value)} style={inputStyle} />
        <label className="hint">matcher kind</label>
        <select value={matcherKind} onChange={(e) => setMatcherKind(e.target.value as MatcherKind)} style={inputStyle}>
          {MATCHERS.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
        <label className="hint">matcher value</label>
        <input
          value={matcherValue}
          onChange={(e) => setMatcherValue(e.target.value)}
          placeholder="/Users/me/work"
          style={inputStyle}
          autoFocus
        />
        <label className="hint">target</label>
        <select value={targetId} onChange={(e) => setTargetId(e.target.value)} style={inputStyle}>
          {targets.length === 0 && <option value="">(no targets — add one first)</option>}
          {targets.map((t) => (
            <option key={t.id} value={t.id}>
              {t.id} · {t.agent}/{t.profile}
            </option>
          ))}
        </select>
        <label className="hint">policy profile</label>
        <input value={policyProfile} onChange={(e) => setPolicyProfile(e.target.value)} style={inputStyle} />
      </div>
      {error && <div style={errorStyle}>{error}</div>}
      <div style={{ display: "flex", gap: 8, marginTop: 12, justifyContent: "flex-end" }}>
        <button type="button" className="btn ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={busy || targets.length === 0}>
          {busy ? "adding…" : "binding add"}
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
