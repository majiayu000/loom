import { useState } from "react";
import type { Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon } from "../../components/icons/nav_icons";
import { TargetAddForm } from "../../components/panel/forms/TargetAddForm";

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
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}
