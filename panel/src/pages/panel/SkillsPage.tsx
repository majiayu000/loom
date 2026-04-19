import { useState } from "react";
import type { Binding, Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon, SearchIcon } from "../../components/icons/nav_icons";
import { api } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface SkillsPageProps {
  skills: Skill[];
  targets: Target[];
  bindings: Binding[];
  selectedSkill: string | null;
  onSelectSkill: (id: string) => void;
  onMutation: () => void;
  readOnly: boolean;
}

export function SkillsPage({ skills, targets, bindings, selectedSkill, onSelectSkill, onMutation, readOnly }: SkillsPageProps) {
  const [q, setQ] = useState("");
  const filtered = skills.filter((s) => s.name.includes(q) || s.tag.includes(q));
  const sel = skills.find((s) => s.id === selectedSkill) ?? skills[0];
  const capture = useMutation();

  const runCapture = () => {
    const skillName = sel?.name;
    capture.run(
      `capture ${skillName ?? "all pending"}`,
      () => api.capture(skillName ? { skill: skillName } : {}),
      onMutation,
    );
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Skills</h1>
          <div className="subtitle">
            Versioned units in the registry. Each skill owns a chain of captures, releases, snapshots.
          </div>
        </div>
        <div className="header-actions">
          <div className="searchbar">
            <SearchIcon />
            <input placeholder="Filter skills…" value={q} onChange={(e) => setQ(e.target.value)} />
            <kbd>⌘K</kbd>
          </div>
          <button className="btn primary" onClick={runCapture} disabled={capture.busy || readOnly} title={readOnly ? "registry offline" : undefined}>
            <PlusIcon /> {capture.busy ? "capturing…" : "Capture"}
          </button>
        </div>
      </div>
      {(capture.error || capture.success) && (
        <div
          style={{
            padding: "6px 28px",
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            borderBottom: "1px solid var(--line)",
            color: capture.error ? "var(--err)" : "var(--ok)",
            background: capture.error ? "rgba(216,90,90,0.08)" : "rgba(111,183,138,0.08)",
          }}
        >
          {capture.error ?? `✓ ${capture.success}`}
        </div>
      )}
      <div className="page-body" style={{ padding: 0 }}>
        <div className="two-col" style={{ height: "100%", gap: 0 }}>
          <div style={{ overflow: "auto", borderRight: "1px solid var(--line)" }}>
            <table className="tbl">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Tag</th>
                  <th>Latest rev</th>
                  <th>Rules</th>
                  <th>Targets</th>
                  <th>Changed</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((s) => (
                  <tr
                    key={s.id}
                    className={sel?.id === s.id ? "selected" : ""}
                    onClick={() => onSelectSkill(s.id)}
                  >
                    <td className="name">{s.name}</td>
                    <td>
                      <span className="chip">{s.tag}</span>
                    </td>
                    <td className="mono">{s.latestRev}</td>
                    <td className="mono dim">{s.ruleCount}</td>
                    <td className="mono">{s.targets.length}</td>
                    <td className="mono dim">{s.changed}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div style={{ padding: 20, overflow: "auto" }}>
            {sel ? (
              <SkillDetail skill={sel} targets={targets} bindings={bindings} />
            ) : (
              <div className="empty">Select a skill.</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

type DetailTab = "history" | "diff" | "targets";

function summarizePolicy(skillBindings: Binding[]): string {
  if (skillBindings.length === 0) return "— (no bindings)";
  const counts = skillBindings.reduce<Record<string, number>>((acc, b) => {
    acc[b.policy] = (acc[b.policy] ?? 0) + 1;
    return acc;
  }, {});
  const kinds = Object.keys(counts);
  if (kinds.length === 1) return `${kinds[0]} · ${skillBindings.length} binding${skillBindings.length === 1 ? "" : "s"}`;
  return kinds.map((k) => `${counts[k]} ${k}`).join(" · ");
}

interface LifecycleEvent {
  kind: "release" | "capture" | "save" | "snapshot" | "project";
  v: string;
  time: string;
  who: string;
  desc: string;
}

const KIND_COLOR: Record<LifecycleEvent["kind"], string> = {
  release: "var(--accent)",
  capture: "var(--pending)",
  save: "var(--ink-2)",
  snapshot: "var(--warn)",
  project: "var(--ok)",
};

// Lifecycle events come from the panel API (per-skill observation stream);
// until that endpoint ships (Wave 3) the component renders an empty state.

function SkillDetail({ skill, targets, bindings }: { skill: Skill; targets: Target[]; bindings: Binding[] }) {
  const [tab, setTab] = useState<DetailTab>("history");
  const targetObjs = skill.targets
    .map((tid) => targets.find((t) => t.id === tid))
    .filter((t): t is Target => t !== undefined);

  const skillBindings = bindings.filter((b) => b.skill === skill.name);
  const policyLabel = summarizePolicy(skillBindings);

  return (
    <div className="detail">
      <h4>{skill.name}</h4>
      <div className="dpath">skills/{skill.name}@{skill.latestRev}</div>
      <div className="kv">
        <div className="k">Tag</div>
        <div className="v">{skill.tag}</div>
        <div className="k">Latest rev</div>
        <div className="v">{skill.latestRev}</div>
        <div className="k">Rules</div>
        <div className="v">{skill.ruleCount} on chain</div>
        <div className="k">Policy</div>
        <div className="v">{policyLabel}</div>
      </div>

      <div className="tabs">
        <button className={tab === "history" ? "active" : ""} onClick={() => setTab("history")}>
          Lifecycle
        </button>
        <button className={tab === "diff" ? "active" : ""} onClick={() => setTab("diff")}>
          Diff
        </button>
        <button className={tab === "targets" ? "active" : ""} onClick={() => setTab("targets")}>
          Targets ({targetObjs.length})
        </button>
      </div>

      {tab === "history" && <Lifecycle events={[]} skillName={skill.name} />}
      {tab === "diff" && <DiffEmpty />}
      {tab === "targets" && <TargetsTab targets={targetObjs} />}
    </div>
  );
}

function Lifecycle({ events, skillName }: { events: LifecycleEvent[]; skillName: string }) {
  if (events.length === 0) {
    return (
      <div style={{ padding: "18px 4px", fontSize: 12, color: "var(--ink-2)" }}>
        <div style={{ marginBottom: 6 }}>No lifecycle events yet.</div>
        <div className="mono" style={{ fontSize: 11, color: "var(--ink-3)" }}>
          Run <span style={{ color: "var(--ink-1)" }}>loom capture {skillName}</span> to start the chain.
        </div>
      </div>
    );
  }
  return (
    <div style={{ position: "relative", paddingLeft: 22 }}>
      <div style={{ position: "absolute", left: 7, top: 4, bottom: 4, width: 1, background: "var(--line)" }} />
      {events.map((e, i) => (
        <div key={i} style={{ position: "relative", marginBottom: 14 }}>
          <div
            style={{
              position: "absolute",
              left: -22,
              top: 4,
              width: 15,
              height: 15,
              borderRadius: 8,
              background: "var(--bg-0)",
              border: `2px solid ${KIND_COLOR[e.kind]}`,
            }}
          />
          <div style={{ fontSize: 12 }}>
            <span style={{ color: "var(--ink-0)", fontWeight: 500 }}>{e.kind}</span>
            <span className="mono" style={{ color: "var(--ink-2)", marginLeft: 6 }}>
              {e.v}
            </span>
            <span style={{ color: "var(--ink-3)", marginLeft: 8 }}>
              by {e.who} · {e.time}
            </span>
          </div>
          <div style={{ fontSize: 11.5, color: "var(--ink-2)", marginTop: 2 }}>{e.desc}</div>
        </div>
      ))}
    </div>
  );
}

function DiffEmpty() {
  return (
    <div style={{ padding: "18px 4px", fontSize: 12, color: "var(--ink-2)" }}>
      <div style={{ marginBottom: 6 }}>No diff available.</div>
      <div className="mono" style={{ fontSize: 11, color: "var(--ink-3)" }}>
        Per-revision diff view coming in v1.0. Use{" "}
        <span style={{ color: "var(--ink-1)" }}>git -C skills/&lt;name&gt; diff</span> for now.
      </div>
    </div>
  );
}

function TargetsTab({ targets }: { targets: Target[] }) {
  return (
    <div>
      {targets.map((t) => (
        <div
          key={t.id}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 12px",
            borderBottom: "1px solid var(--line-soft)",
          }}
        >
          <AgentAvatar agent={t.agent} />
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 12.5, color: "var(--ink-0)" }}>
              {t.agent}/{t.profile}
            </div>
            <div className="mono" style={{ fontSize: 10.5, color: "var(--ink-3)" }}>
              {t.path}
            </div>
          </div>
          <span className={`chip ${t.ownership}`}>
            <span className="dot" />
            {t.ownership}
          </span>
        </div>
      ))}
    </div>
  );
}
