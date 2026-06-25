import { useState } from "react";
import { MutationBanner } from "../../components/panel/MutationBanner";
import { AGENT_OPTIONS } from "../../lib/agent_options";
import { api } from "../../lib/api/client";
import type { Target } from "../../lib/types";
import { useMutation } from "../../lib/useMutation";

type ProjectionMethod = "symlink" | "copy" | "materialize";

export function UseSkillForm({
  skillName,
  targets,
  onMutation,
  readOnly,
}: {
  skillName: string;
  targets: Target[];
  onMutation: () => void;
  readOnly: boolean;
}) {
  const [agents, setAgents] = useState(() => defaultUseAgents(targets));
  const [workspace, setWorkspace] = useState("");
  const [profile, setProfile] = useState("default");
  const [method, setMethod] = useState<ProjectionMethod>("symlink");
  const [planSummary, setPlanSummary] = useState("");
  const useSkill = useMutation();
  const disabled = readOnly || useSkill.busy || agents.length === 0;

  const toggleAgent = (agent: string) => {
    setAgents((current) =>
      current.includes(agent) ? current.filter((item) => item !== agent) : [...current, agent],
    );
  };

  const runUse = (apply: boolean) => {
    useSkill.run(
      apply ? "use apply" : "use plan",
      () =>
        api.skillUse(skillName, {
          agents,
          scope: "project",
          workspace: workspace.trim() || undefined,
          profile: profile.trim() || "default",
          method,
          apply,
        }),
      (result) => {
        const data = (result as { data?: { steps?: unknown[] } }).data;
        if (apply) {
          setPlanSummary("");
          onMutation();
          return;
        }
        const count = Array.isArray(data?.steps) ? data.steps.length : 0;
        setPlanSummary(`${count} agent${count === 1 ? "" : "s"} planned`);
      },
    );
  };

  return (
    <div className="card" style={{ padding: 12, marginBottom: 12 }}>
      <div style={{ display: "grid", gap: 8 }}>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }} role="group" aria-label="Use agents">
          {AGENT_OPTIONS.map((option) => (
            <label key={option.slug} className="chip" style={{ cursor: readOnly ? "default" : "pointer" }}>
              <input
                type="checkbox"
                checked={agents.includes(option.slug)}
                onChange={() => toggleAgent(option.slug)}
                disabled={readOnly || useSkill.busy}
                style={{ marginRight: 5 }}
              />
              {option.label}
            </label>
          ))}
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.4fr) minmax(0, 0.8fr) 130px auto auto", gap: 8 }}>
          <input
            value={workspace}
            onChange={(event) => setWorkspace(event.target.value)}
            placeholder="workspace path"
            style={formInputStyle}
            disabled={readOnly || useSkill.busy}
          />
          <input
            value={profile}
            onChange={(event) => setProfile(event.target.value)}
            placeholder="profile"
            style={formInputStyle}
            disabled={readOnly || useSkill.busy}
          />
          <select
            value={method}
            onChange={(event) => setMethod(event.target.value as ProjectionMethod)}
            style={formInputStyle}
            disabled={readOnly || useSkill.busy}
          >
            <option value="symlink">symlink</option>
            <option value="copy">copy</option>
            <option value="materialize">materialize</option>
          </select>
          <button className="btn ghost" type="button" onClick={() => runUse(false)} disabled={disabled}>
            Plan
          </button>
          <button className="btn primary" type="button" onClick={() => runUse(true)} disabled={disabled}>
            {useSkill.busy ? "running..." : "Apply"}
          </button>
        </div>
      </div>
      {planSummary && <div className="mono dim" style={{ fontSize: 11, marginTop: 8 }}>{planSummary}</div>}
      <MutationBanner state={useSkill} spacing="top" />
    </div>
  );
}

function defaultUseAgents(targets: Target[]): string[] {
  const known = new Set(AGENT_OPTIONS.map((option) => option.slug));
  const targetAgents = Array.from(new Set(targets.map((target) => target.agent).filter((agent) => known.has(agent))));
  return targetAgents.length > 0 ? targetAgents.slice(0, 2) : ["claude"];
}

const formInputStyle: React.CSSProperties = {
  padding: "6px 10px",
  borderRadius: 6,
  border: "1px solid var(--line)",
  background: "var(--bg-1)",
  color: "var(--ink-0)",
  fontSize: 12,
  minWidth: 0,
};
