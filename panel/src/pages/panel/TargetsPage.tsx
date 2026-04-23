import { useState } from "react";
import type { Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon } from "../../components/icons/nav_icons";
import { TargetAddForm } from "../../components/panel/forms/TargetAddForm";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface TargetsPageProps {
  targets: Target[];
  skills: Skill[];
  selectedTarget: string | null;
  onSelectTarget: (id: string) => void;
  onMutation: () => void;
  readOnly: boolean;
}

export function TargetsPage({ targets, skills, selectedTarget, onSelectTarget, onMutation, readOnly }: TargetsPageProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const remove = useMutation();
  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Targets</h1>
          <div className="subtitle">
            Agent skill directories Loom knows about. Ownership determines whether Loom writes, reads, or stays
            hands-off.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn primary"
            onClick={() => setAddOpen((v) => !v)}
            disabled={readOnly}
            title={readOnly ? "registry offline" : undefined}
          >
            <PlusIcon /> {addOpen ? "close" : "target add"}
          </button>
        </div>
      </div>
      <div className="page-body">
        {(remove.error || remove.success) && (
          <div
            style={{
              marginBottom: 12,
              padding: "8px 12px",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              border: "1px solid",
              borderColor: remove.error ? "rgba(216,90,90,0.28)" : "rgba(111,183,138,0.28)",
              color: remove.error ? "var(--err)" : "var(--ok)",
              background: remove.error ? "rgba(216,90,90,0.08)" : "rgba(111,183,138,0.08)",
              borderRadius: 10,
            }}
          >
            {remove.error ?? `✓ ${remove.success}`}
          </div>
        )}
        {addOpen && (
          <TargetAddForm
            onCancel={() => setAddOpen(false)}
            onSuccess={() => {
              setAddOpen(false);
              onMutation();
            }}
          />
        )}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: 12 }}>
          {targets.map((t) => {
            const sel = selectedTarget === t.id;
            const inbound = skills.filter((s) => s.targets.includes(t.id)).length;
            return (
              <div
                key={t.id}
                className="card"
                style={{ cursor: "pointer", borderColor: sel ? "var(--accent)" : "var(--line)" }}
                onClick={() => onSelectTarget(t.id)}
              >
                <div style={{ padding: "14px 16px", display: "flex", alignItems: "center", gap: 12 }}>
                  <AgentAvatar agent={t.agent} size={32} radius={8} />
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 14, color: "var(--ink-0)", fontWeight: 500 }}>
                      {t.agent}
                      <span style={{ color: "var(--ink-3)" }}> / </span>
                      {t.profile}
                    </div>
                    <div className="mono" style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 2 }}>
                      {t.path}
                    </div>
                  </div>
                  <span className={`chip ${t.ownership}`}>
                    <span className="dot" />
                    {t.ownership}
                  </span>
                </div>
                <div
                  style={{
                    padding: "10px 16px",
                    borderTop: "1px solid var(--line-soft)",
                    display: "flex",
                    gap: 18,
                    fontSize: 11.5,
                    color: "var(--ink-2)",
                  }}
                >
                  <span>
                    <b style={{ color: "var(--ink-0)" }}>{t.skills}</b> skills present
                  </span>
                  <span>
                    <b style={{ color: "var(--ink-0)" }}>{inbound}</b> inbound bindings
                  </span>
                  <span style={{ marginLeft: "auto", color: "var(--ink-3)" }}>synced {t.lastSync}</span>
                </div>
                <div
                  style={{
                    padding: "10px 16px",
                    borderTop: "1px solid var(--line-soft)",
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    gap: 10,
                  }}
                  onClick={(e) => e.stopPropagation()}
                >
                  <span style={{ fontSize: 11, color: "var(--ink-3)" }} className="mono">
                    {t.id}
                  </span>
                  {confirmingId === t.id ? (
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 11, color: "var(--ink-2)" }}>
                        remove this target?
                      </span>
                      <button className="btn sm" onClick={() => setConfirmingId(null)} disabled={remove.busy}>
                        Cancel
                      </button>
                      <button
                        className="btn sm"
                        style={{ color: "var(--err)", borderColor: "rgba(216,90,90,0.3)" }}
                        disabled={remove.busy || readOnly}
                        onClick={() =>
                          remove.run(`remove ${t.id}`, () => api.targetRemove(t.id), () => {
                            setConfirmingId(null);
                            onMutation();
                          })
                        }
                      >
                        {remove.busy ? "removing…" : "Remove"}
                      </button>
                    </div>
                  ) : (
                    <button
                      className="btn sm"
                      disabled={readOnly}
                      onClick={() => setConfirmingId(t.id)}
                      title={readOnly ? "registry offline" : undefined}
                    >
                      Remove
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}
