import { useEffect, useState, type CSSProperties } from "react";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import type { Binding, Target } from "../../lib/types";

export function SkillTargetsTab({
  skillName,
  bindings,
  targets,
  projectedTargetIds,
  onMutation,
  readOnly,
}: {
  skillName: string;
  bindings: Binding[];
  targets: Target[];
  projectedTargetIds: string[];
  onMutation: () => void;
  readOnly: boolean;
}) {
  const targetObjs = targets.filter((target) => projectedTargetIds.includes(target.id));
  return (
    <>
      <ProjectSkillForm
        skillName={skillName}
        bindings={bindings}
        targets={targets}
        onMutation={onMutation}
        readOnly={readOnly}
      />
      <TargetsTab targets={targetObjs} />
    </>
  );
}

function ProjectSkillForm({
  skillName,
  bindings,
  targets,
  onMutation,
  readOnly,
}: {
  skillName: string;
  bindings: Binding[];
  targets: Target[];
  onMutation: () => void;
  readOnly: boolean;
}) {
  const [bindingId, setBindingId] = useState(bindings[0]?.id ?? "");
  const [method, setMethod] = useState<"symlink" | "copy" | "materialize">("symlink");
  const project = useMutation();
  const bindingKey = bindings.map((binding) => binding.id).join("\0");

  useEffect(() => {
    setBindingId((current) => {
      if (bindings.some((binding) => binding.id === current)) return current;
      return bindings[0]?.id ?? "";
    });
    // Recompute only when the selectable binding set changes, not on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bindingKey]);

  const runProject = () => {
    if (!bindingId) return;
    project.run("skill project", () => api.project({ skill: skillName, binding: bindingId, method }), onMutation);
  };

  return (
    <div className="card" style={{ padding: 12, marginBottom: 12 }}>
      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 130px auto", gap: 8 }}>
        <select value={bindingId} onChange={(event) => setBindingId(event.target.value)} style={formInputStyle} disabled={readOnly}>
          {bindings.length === 0 && <option value="">(no bindings)</option>}
          {bindings.map((binding) => {
            const target = targets.find((item) => item.id === binding.target);
            return (
              <option key={binding.id} value={binding.id}>
                {binding.id} · {target ? `${target.agent}/${target.profile}` : binding.target}
              </option>
            );
          })}
        </select>
        <select
          value={method}
          onChange={(event) => setMethod(event.target.value as "symlink" | "copy" | "materialize")}
          style={formInputStyle}
          disabled={readOnly}
        >
          <option value="symlink">symlink</option>
          <option value="copy">copy</option>
          <option value="materialize">materialize</option>
        </select>
        <button className="btn primary" onClick={runProject} disabled={readOnly || project.busy || !bindingId}>
          {project.busy ? "projecting…" : "Project"}
        </button>
      </div>
      {(project.error || project.success) && <div style={project.error ? errorStyle : okStyle}>{project.error ?? `✓ ${project.success}`}</div>}
    </div>
  );
}

function TargetsTab({ targets }: { targets: Target[] }) {
  if (targets.length === 0) {
    return <div className="empty" style={{ padding: "18px 4px" }}>This skill is not projected to any target.</div>;
  }

  return (
    <div>
      {targets.map((target) => (
        <div
          key={target.id}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 12px",
            borderBottom: "1px solid var(--line-soft)",
          }}
        >
          <AgentAvatar agent={target.agent} />
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 12.5, color: "var(--ink-0)" }}>
              {target.agent}/{target.profile}
            </div>
            <div className="mono" style={{ fontSize: 10.5, color: "var(--ink-3)" }}>
              {target.path}
            </div>
          </div>
          <span className={`chip ${target.ownership}`}>
            <span className="dot" />
            {target.ownership}
          </span>
        </div>
      ))}
    </div>
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
