import { useState } from "react";
import type { Binding, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon } from "../../components/icons/nav_icons";
import { BindingAddForm } from "../../components/panel/forms/BindingAddForm";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface BindingsPageProps {
  bindings: Binding[];
  targets: Target[];
  onMutation: () => void;
  readOnly: boolean;
}

export function BindingsPage({ bindings, targets, onMutation, readOnly }: BindingsPageProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const remove = useMutation();
  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Bindings</h1>
          <div className="subtitle">
            Rules mapping skills to targets. Matchers decide when a binding applies; policy decides whether Loom
            auto-projects.
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn primary"
            onClick={() => setAddOpen((v) => !v)}
            disabled={readOnly}
            title={readOnly ? "registry offline" : undefined}
          >
            <PlusIcon /> {addOpen ? "close" : "New binding"}
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
          <BindingAddForm
            targets={targets}
            onCancel={() => setAddOpen(false)}
            onSuccess={() => {
              setAddOpen(false);
              onMutation();
            }}
          />
        )}
        <table
          className="tbl"
          style={{
            background: "var(--bg-1)",
            borderRadius: 10,
            overflow: "hidden",
            border: "1px solid var(--line)",
          }}
        >
          <thead>
            <tr>
              <th>Binding</th>
              <th>Skill</th>
              <th>Target</th>
              <th>Matcher</th>
              <th>Method</th>
              <th>Policy</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {bindings.map((b) => {
              const t = targets.find((x) => x.id === b.target);
              return (
                <tr key={b.id}>
                  <td className="mono dim">{b.id}</td>
                  <td className="name">{b.skill}</td>
                  <td>
                    {t && (
                      <span className="row-flex">
                        <AgentAvatar agent={t.agent} />
                        <span style={{ color: "var(--ink-1)" }}>
                          {t.agent}/{t.profile}
                        </span>
                      </span>
                    )}
                  </td>
                  <td className="mono">{b.matcher}</td>
                  <td>
                    <span className={`chip method ${b.method}`}>{b.method}</span>
                  </td>
                  <td>
                    <span
                      className="chip"
                      style={{ color: b.policy === "auto" ? "var(--ok)" : "var(--warn)" }}
                    >
                      {b.policy}
                    </span>
                  </td>
                  <td style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                    {confirmingId === b.id ? (
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                        <span style={{ fontSize: 11, color: "var(--ink-2)" }}>remove?</span>
                        <button className="btn sm" onClick={() => setConfirmingId(null)} disabled={remove.busy}>
                          Cancel
                        </button>
                        <button
                          className="btn sm"
                          style={{ color: "var(--err)", borderColor: "rgba(216,90,90,0.3)" }}
                          disabled={remove.busy || readOnly}
                          onClick={() =>
                            remove.run(`remove ${b.id}`, () => api.bindingRemove(b.id), () => {
                              setConfirmingId(null);
                              onMutation();
                            })
                          }
                        >
                          {remove.busy ? "removing…" : "Remove"}
                        </button>
                      </span>
                    ) : (
                      <button
                        className="btn sm"
                        disabled={readOnly}
                        onClick={() => setConfirmingId(b.id)}
                        title={readOnly ? "registry offline" : undefined}
                      >
                        Remove
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}
