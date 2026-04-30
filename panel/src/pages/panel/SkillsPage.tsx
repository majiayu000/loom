import { useEffect, useState } from "react";
import type { Binding, Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon, SearchIcon } from "../../components/icons/nav_icons";
import { api, type SkillDiffFile, type RegistryObservationEvent } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface SkillsPageProps {
  skills: Skill[];
  targets: Target[];
  bindings?: Binding[];
  selectedSkill: string | null;
  onSelectSkill: (id: string) => void;
  onMutation: () => void;
  readOnly: boolean;
}

export function SkillsPage({ skills, targets, bindings = [], selectedSkill, onSelectSkill, onMutation, readOnly }: SkillsPageProps) {
  const [q, setQ] = useState("");
  const [addOpen, setAddOpen] = useState(false);
  const filtered = skills.filter((s) => s.name.includes(q) || s.tag.includes(q));
  const sel = skills.find((s) => s.id === selectedSkill) ?? skills[0];
  const capture = useMutation();
  const captureDisabled = capture.busy || readOnly || !sel;
  const emptyMessage = readOnly
    ? "Live registry API is offline. Start the panel backend to load real skills."
    : q
    ? "No skills match the current filter."
    : "No skills in this registry yet.";

  const runCapture = () => {
    if (!sel) return;
    const skillName = sel?.name;
    capture.run(
      `capture ${skillName}`,
      () => api.capture({ skill: skillName }),
      onMutation,
    );
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Skills</h1>
          <div className="subtitle">
            Tracked units in the registry. Each skill owns a chain of captures, releases, snapshots.
          </div>
        </div>
        <div className="header-actions">
          <div className="searchbar">
            <SearchIcon />
            <input placeholder="Filter skills…" value={q} onChange={(e) => setQ(e.target.value)} />
            <kbd>⌘K</kbd>
          </div>
          <button
            className="btn primary"
            onClick={runCapture}
            disabled={captureDisabled}
            title={readOnly ? "registry offline" : !sel ? "select a skill first" : undefined}
          >
            <PlusIcon /> {capture.busy ? "capturing…" : "Capture"}
          </button>
          <button
            className="btn primary"
            onClick={() => setAddOpen((value) => !value)}
            disabled={readOnly}
            title={readOnly ? "registry offline" : undefined}
          >
            <PlusIcon /> {addOpen ? "close" : "skill add"}
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
        {addOpen && (
          <div style={{ padding: "0 28px 12px" }}>
            <SkillAddForm
              onCancel={() => setAddOpen(false)}
              onSuccess={() => {
                setAddOpen(false);
                onMutation();
              }}
            />
          </div>
        )}
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
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={6} style={{ color: "var(--ink-3)", padding: 22, textAlign: "center" }}>
                      {emptyMessage}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          <div style={{ padding: 20, overflow: "auto" }}>
            {sel ? (
              <SkillDetail skill={sel} targets={targets} bindings={bindings} onMutation={onMutation} readOnly={readOnly} />
            ) : (
              <div className="empty">{emptyMessage}</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

function SkillAddForm({ onCancel, onSuccess }: { onCancel: () => void; onSuccess: () => void }) {
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const add = useMutation();

  const submit = (event: React.FormEvent) => {
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
        <input value={name} onChange={(event) => setName(event.target.value)} placeholder="my-skill" style={formInputStyle} />
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

type DetailTab = "history" | "diff" | "targets";

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

const KIND_MAP: Record<string, LifecycleEvent["kind"]> = {
  captured: "capture",
  projected: "project",
  snapshot: "snapshot",
  released: "release",
  saved: "save",
  file_changed: "save",
  health_changed: "snapshot",
};

function toRelative(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

function mapObsToLifecycle(ev: RegistryObservationEvent): LifecycleEvent {
  return {
    kind: KIND_MAP[ev.kind] ?? "capture",
    v: ev.event_id.slice(0, 8),
    time: toRelative(ev.observed_at),
    who: ev.instance_id.slice(0, 8),
    desc: ev.path ?? (ev.from && ev.to ? `${ev.from} → ${ev.to}` : ev.kind),
  };
}


function SkillDetail({
  skill,
  targets,
  bindings,
  onMutation,
  readOnly,
}: {
  skill: Skill;
  targets: Target[];
  bindings: Binding[];
  onMutation: () => void;
  readOnly: boolean;
}) {
  const [tab, setTab] = useState<DetailTab>("history");
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyEvents, setHistoryEvents] = useState<LifecycleEvent[]>([]);

  const targetObjs = skill.targets
    .map((tid) => targets.find((t) => t.id === tid))
    .filter((t): t is Target => t !== undefined);

  const skillBindings = bindings.filter((b) => b.skill === skill.name);
  const policyLabel = summarizePolicy(skillBindings);

  useEffect(() => {
    if (tab !== "history") return;
    const ctrl = new AbortController();
    setHistoryLoading(true);
    setHistoryError(null);
    api
      .skillHistory(skill.name, ctrl.signal)
      .then((payload) => {
        setHistoryEvents(payload.data?.events.map(mapObsToLifecycle) ?? []);
        setHistoryLoading(false);
      })
      .catch((err: Error) => {
        if (err.name !== "AbortError") {
          setHistoryError(err.message);
          setHistoryLoading(false);
        }
      });
    return () => ctrl.abort();
  }, [skill.name, skill.latestRev, tab]);

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

      {tab === "history" && (
        <>
          {historyLoading && (
            <div style={{ color: "var(--ink-3)", fontSize: 12 }}>Loading…</div>
          )}
          {historyError && (
            <div style={{ color: "var(--err)", fontSize: 11, fontFamily: "var(--font-mono)" }}>
              {historyError}
            </div>
          )}
          {!historyLoading && !historyError && (
            <Lifecycle events={historyEvents} skillName={skill.name} />
          )}
        </>
      )}
      {tab === "diff" && <SkillDiff skillName={skill.name} />}
      {tab === "targets" && (
        <>
          <ProjectSkillForm
            skillName={skill.name}
            bindings={bindings}
            targets={targets}
            onMutation={onMutation}
            readOnly={readOnly}
          />
          <TargetsTab targets={targetObjs} />
        </>
      )}
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

function SkillDiff({ skillName }: { skillName: string }) {
  const [revA, setRevA] = useState("");
  const [revB, setRevB] = useState("");
  const [files, setFiles] = useState<SkillDiffFile[] | null>(null);
  const [header, setHeader] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    api
      .skillDiff(skillName, revA || undefined, revB || undefined, ctrl.signal)
      .then((payload) => {
        if (payload.data) {
          setFiles(payload.data.files);
          setHeader(`${payload.data.rev_a.slice(0, 7)} → ${payload.data.rev_b.slice(0, 7)}`);
        }
        setLoading(false);
      })
      .catch((err: Error) => {
        if (err.name !== "AbortError") {
          setError(err.message);
          setFiles(null);
          setHeader("");
          setLoading(false);
        }
      });
    return () => ctrl.abort();
  }, [skillName, revA, revB]);

  const inputStyle: React.CSSProperties = {
    fontSize: 11,
    padding: "2px 6px",
    background: "var(--bg-1)",
    border: "1px solid var(--line)",
    borderRadius: 4,
    color: "var(--ink-0)",
    width: 130,
    fontFamily: "var(--font-mono)",
  };

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <input
          style={inputStyle}
          placeholder="rev_a (default: prev)"
          value={revA}
          onChange={(e) => setRevA(e.target.value)}
        />
        <span style={{ color: "var(--ink-3)", fontSize: 11 }}>→</span>
        <input
          style={inputStyle}
          placeholder="rev_b (default: HEAD)"
          value={revB}
          onChange={(e) => setRevB(e.target.value)}
        />
      </div>

      {loading && (
        <div style={{ color: "var(--ink-3)", fontSize: 12 }}>Loading diff…</div>
      )}
      {error && (
        <div style={{ color: "var(--err)", fontSize: 11, fontFamily: "var(--font-mono)" }}>
          {error}
        </div>
      )}

      {!loading && files !== null && (
        <>
          {header && <div className="section-title">{header}</div>}
          {files.length === 0 ? (
            <div style={{ color: "var(--ink-3)", fontSize: 12 }}>
              No changes in skills/{skillName}/
            </div>
          ) : (
            files.map((file) => (
              <div key={file.path} style={{ marginBottom: 16 }}>
                <div
                  className="mono"
                  style={{ fontSize: 11, color: "var(--ink-2)", marginBottom: 4 }}
                >
                  {file.path}{" "}
                  <span style={{ color: "var(--ok)" }}>+{file.added}</span>{" "}
                  <span style={{ color: "var(--err)" }}>-{file.removed}</span>
                </div>
                <div style={{ border: "1px solid var(--line)", borderRadius: 6, overflow: "hidden" }}>
                  {file.hunks.map((hunk, hi) => (
                    <div key={hi}>
                      <div className="diff-row" style={{ background: "var(--bg-1)" }}>
                        <div className="mark" />
                        <div className="l" style={{ color: "var(--ink-3)" }}>{hunk.header}</div>
                      </div>
                      {hunk.lines.map((line, li) => (
                        <div
                          key={li}
                          className={`diff-row${line.startsWith("+") ? " add" : line.startsWith("-") ? " del" : ""}`}
                        >
                          <div className="mark">
                            {line.startsWith("+") ? "+" : line.startsWith("-") ? "-" : ""}
                          </div>
                          <div className="l">{line.slice(1)}</div>
                        </div>
                      ))}
                    </div>
                  ))}
                </div>
              </div>
            ))
          )}
        </>
      )}
    </div>
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

const formInputStyle: React.CSSProperties = {
  padding: "6px 10px",
  borderRadius: 6,
  border: "1px solid var(--line-hi)",
  background: "var(--bg-2)",
  color: "var(--ink-0)",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  minWidth: 0,
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

const okStyle: React.CSSProperties = {
  ...errorStyle,
  color: "var(--ok)",
  background: "rgba(111,183,138,0.08)",
  border: "1px solid rgba(111,183,138,0.3)",
};
