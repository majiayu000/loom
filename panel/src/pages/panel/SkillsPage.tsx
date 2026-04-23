import { useState, useEffect } from "react";
import type { Skill, Target } from "../../lib/types";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon, SearchIcon } from "../../components/icons/nav_icons";
import { api, type SkillDiffFile, type V3ObservationEvent } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

interface SkillsPageProps {
  skills: Skill[];
  targets: Target[];
  selectedSkill: string | null;
  onSelectSkill: (id: string) => void;
  onMutation: () => void;
  readOnly: boolean;
}

export function SkillsPage({ skills, targets, selectedSkill, onSelectSkill, onMutation, readOnly }: SkillsPageProps) {
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
            {sel ? <SkillDetail skill={sel} targets={targets} /> : <div className="empty">Select a skill.</div>}
          </div>
        </div>
      </div>
    </>
  );
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

function mapObsToLifecycle(ev: V3ObservationEvent): LifecycleEvent {
  return {
    kind: KIND_MAP[ev.kind] ?? "capture",
    v: ev.event_id.slice(0, 8),
    time: toRelative(ev.observed_at),
    who: ev.instance_id.slice(0, 8),
    desc: ev.path ?? (ev.from && ev.to ? `${ev.from} → ${ev.to}` : ev.kind),
  };
}


function SkillDetail({ skill, targets }: { skill: Skill; targets: Target[] }) {
  const [tab, setTab] = useState<DetailTab>("history");
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyEvents, setHistoryEvents] = useState<LifecycleEvent[]>([]);

  const targetObjs = skill.targets
    .map((tid) => targets.find((t) => t.id === tid))
    .filter((t): t is Target => t !== undefined);

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
        <div className="v">auto-project on binding match</div>
      </div>

      <div className="tabs">
        <button className={tab === "history" ? "active" : ""} onClick={() => setTab("history")}>
          Summary
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
            <Lifecycle events={historyEvents} />
          )}
        </>
      )}
      {tab === "diff" && <SkillDiff skillName={skill.name} />}
      {tab === "targets" && <TargetsTab targets={targetObjs} />}
    </div>
  );
}

function Lifecycle({ events }: { events: LifecycleEvent[] }) {
  if (events.length === 0) {
    return (
      <div style={{ color: "var(--ink-3)", fontSize: 12 }}>
        No lifecycle events yet · run <code>loom capture &lt;skill&gt;</code>
      </div>
    );
  }
  return (
    <div style={{ display: "grid", gap: 12 }}>
      <div style={{ border: "1px solid var(--line)", borderRadius: 10, padding: "12px 14px", background: "var(--bg-1)" }}>
        <div style={{ fontSize: 11, letterSpacing: "0.08em", textTransform: "uppercase", color: "var(--ink-3)", marginBottom: 6 }}>
          Current state
        </div>
        <div style={{ color: "var(--ink-1)", fontSize: 12.5 }}>
          This view is backed by the current projection snapshot. It shows the latest applied revision, target coverage, and rule count without inventing lifecycle events the backend does not expose yet.
        </div>
      </div>
      <div className="kv" style={{ gridTemplateColumns: "130px 1fr" }}>
        <div className="k">latest revision</div>
        <div className="v mono">{skill.latestRev}</div>
        <div className="k">last changed</div>
        <div className="v">{skill.changed}</div>
        <div className="k">rule count</div>
        <div className="v">{skill.ruleCount}</div>
        <div className="k">projected targets</div>
        <div className="v">{targetCount}</div>
        <div className="k">classification</div>
        <div className="v">{skill.tag}</div>
      </div>
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
