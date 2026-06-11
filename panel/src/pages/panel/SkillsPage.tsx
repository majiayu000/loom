import { useEffect, useState } from "react";
import type { Binding, Skill, Target } from "../../lib/types";
import type { RegistryProjection } from "../../generated/RegistryProjection";
import { AgentAvatar } from "../../components/panel/AgentAvatar";
import { PlusIcon, SearchIcon, SkillIcon } from "../../components/icons/nav_icons";
import { EmptyState } from "../../components/panel/EmptyState";
import { MutationBanner } from "../../components/panel/MutationBanner";
import { api, type SkillDiagnosePayload } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";
import { SkillDiagnosePanel } from "./SkillDiagnosePanel";
import { SkillTrashPanel, SkillViewModeSwitch, TrashSkillAction } from "./SkillTrashPanel";
import {
  Lifecycle,
  LifecycleActions,
  SkillDiff,
  mapObsToLifecycle,
  type LifecycleEvent,
} from "./SkillLifecycle";

interface SkillsPageProps {
  skills: Skill[];
  targets: Target[];
  bindings?: Binding[];
  projections?: RegistryProjection[];
  selectedSkill: string | null;
  onSelectSkill: (id: string | null) => void;
  onMutation: () => void;
  readOnly: boolean;
}

type SkillsViewMode = "skills" | "trash";

export function SkillsPage({
  skills,
  targets,
  bindings = [],
  projections = [],
  selectedSkill,
  onSelectSkill,
  onMutation,
  readOnly,
}: SkillsPageProps) {
  const [q, setQ] = useState("");
  const [mode, setMode] = useState<SkillsViewMode>("skills");
  const [addOpen, setAddOpen] = useState(false);
  const [captureBindingId, setCaptureBindingId] = useState("");
  const [trashRefreshKey, setTrashRefreshKey] = useState(0);
  const query = q.trim().toLowerCase();
  const filtered = skills.filter((s) => {
    if (!query) return true;
    return [s.name, s.tag, s.description ?? ""].some((value) =>
      value.toLowerCase().includes(query),
    );
  });
  const sel = filtered.find((s) => s.id === selectedSkill) ?? filtered[0] ?? skills.find((s) => s.id === selectedSkill) ?? skills[0];
  const capture = useMutation();
  const selectedSkillBindings = sel ? captureBindingsForSkill(sel.name, bindings, projections) : [];
  const bindingOptionKey = selectedSkillBindings.map((b) => `${b.id}\u001f${b.target}\u001f${b.method}`).join("\u001e");
  const captureBinding = selectedSkillBindings.find((b) => b.id === captureBindingId) ?? null;
  const captureDisabled = capture.busy || readOnly || !sel || !captureBinding;
  const captureTitle = readOnly
    ? "registry offline"
    : !sel
      ? "select a skill first"
      : !captureBinding
        ? "projection required"
        : undefined;

  useEffect(() => {
    if (selectedSkillBindings.length === 0) {
      setCaptureBindingId("");
      return;
    }
    setCaptureBindingId((current) =>
      selectedSkillBindings.some((b) => b.id === current) ? current : selectedSkillBindings[0].id,
    );
  }, [bindingOptionKey]);

  const runCapture = () => {
    if (!sel || !captureBinding) return;
    const skillName = sel?.name;
    capture.run(
      `capture ${skillName}`,
      () => api.capture({ skill: skillName, binding: captureBinding.id }),
      onMutation,
    );
  };

  const onSkillTrashed = () => {
    onSelectSkill(null);
    setTrashRefreshKey((value) => value + 1);
    onMutation();
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
          <SkillViewModeSwitch mode={mode} onModeChange={setMode} />
          <div className="searchbar">
            <SearchIcon />
            <input
              placeholder={mode === "trash" ? "Filter trash…" : "Filter skills…"}
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
            <kbd>⌘K</kbd>
          </div>
          {mode === "skills" && selectedSkillBindings.length > 1 && (
            <select
              aria-label="Capture binding"
              value={captureBindingId}
              onChange={(event) => setCaptureBindingId(event.target.value)}
              disabled={readOnly || capture.busy}
              title="Choose which projected binding to capture from"
              style={captureSelectStyle}
            >
              {selectedSkillBindings.map((binding) => (
                <option key={binding.id} value={binding.id}>
                  {formatCaptureBinding(binding, targets)}
                </option>
              ))}
            </select>
          )}
          {mode === "skills" && (
            <>
              <button
                className="btn primary"
                onClick={runCapture}
                disabled={captureDisabled}
                title={captureTitle}
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
            </>
          )}
        </div>
      </div>
      <MutationBanner state={capture} variant="bar" />
      <div className="page-body" style={{ padding: 0 }}>
        {mode === "skills" && addOpen && (
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
        {mode === "skills" ? (
          filtered.length === 0 ? (
            <SkillListEmptyState
              query={query}
              readOnly={readOnly}
              onAddSkill={() => setAddOpen(true)}
              onClearFilter={() => setQ("")}
            />
          ) : (
            <div className="two-col" style={{ height: "100%", gap: 0 }}>
              <div style={{ overflow: "auto", borderRight: "1px solid var(--line)" }}>
                <table className="tbl mobile-cards">
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>Source</th>
                      <th>Latest rev</th>
                      <th>Tags</th>
                      <th>Bindings</th>
                      <th>Projections</th>
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
                        <td className="name" data-label="Name">
                          <div>{s.name}</div>
                          {s.description && <div style={skillDescriptionStyle}>{s.description}</div>}
                        </td>
                        <td data-label="Source">
                          <span className={`chip ${s.sourceStatus}`}>{s.sourceStatus}</span>
                        </td>
                        <td className="mono" data-label="Latest rev">
                          {s.latestRev}
                        </td>
                        <td className="mono dim mobile-hide" data-label="Tags">
                          {formatSkillTags(s)}
                        </td>
                        <td className="mono dim" data-label="Bindings">
                          {s.bindingCount}
                        </td>
                        <td className="mono" data-label="Projections">
                          {s.projectionCount}
                        </td>
                        <td className="mono dim mobile-hide" data-label="Changed">
                          {s.changed}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div style={{ padding: 20, overflow: "auto" }}>
                {sel ? (
                  <SkillDetail
                    skill={sel}
                    targets={targets}
                    bindings={bindings}
                    onMutation={onMutation}
                    onTrashed={onSkillTrashed}
                    readOnly={readOnly}
                  />
                ) : (
                  <div className="empty">Select a skill.</div>
                )}
              </div>
            </div>
          )
        ) : (
          <SkillTrashPanel
            query={query}
            refreshKey={trashRefreshKey}
            readOnly={readOnly}
            onMutation={onMutation}
          />
        )}
      </div>
    </>
  );
}

function SkillListEmptyState({
  query,
  readOnly,
  onAddSkill,
  onClearFilter,
}: {
  query: string;
  readOnly: boolean;
  onAddSkill: () => void;
  onClearFilter: () => void;
}) {
  if (query) {
    return (
      <EmptyState
        title="No matching skills"
        icon={<SearchIcon />}
        actions={[{ label: "Clear filter", onClick: onClearFilter, variant: "ghost" }]}
      >
        Nothing in the registry matches <span className="mono">{query}</span>.
      </EmptyState>
    );
  }

  if (readOnly) {
    return (
      <EmptyState title="Registry API offline" icon={<SkillIcon />}>
        Skills need the live registry API. Start the panel again, then add or import skills from this page.
      </EmptyState>
    );
  }

  return (
    <EmptyState
      title="No tracked skills yet"
      icon={<SkillIcon />}
      command="loom skill add <source> --name <name>"
      actions={[{ label: "+ skill add", onClick: onAddSkill }]}
    >
      Add a managed skill manually, or run <span className="mono">loom skill import-observed</span> to pull observed skill directories.
    </EmptyState>
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
      <MutationBanner state={add} spacing="top" />
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

function captureBindingsForSkill(
  skillName: string,
  bindings: Binding[],
  projections: RegistryProjection[],
): Binding[] {
  return bindings.filter(
    (binding) =>
      binding.skill === skillName ||
      projections.some(
        (projection) => projection.skill_id === skillName && projection.binding_id === binding.id,
      ),
  );
}

function formatSkillTags(skill: Skill): string {
  const tags = [
    ...skill.releaseTags.map((tag) => `release:${tag}`),
    ...skill.snapshotTags.map((tag) => `snapshot:${tag}`),
  ];
  if (tags.length === 0) return "—";
  if (tags.length <= 2) return tags.join(" ");
  return `${tags[0]} +${tags.length - 1}`;
}

function formatCaptureBinding(binding: Binding, targets: Target[]): string {
  const target = targets.find((t) => t.id === binding.target);
  const targetLabel = target ? `${target.agent}/${target.profile}` : binding.target;
  return `${targetLabel} · ${binding.method} · ${binding.policy}`;
}

const captureSelectStyle = {
  height: 32,
  minWidth: 190,
  maxWidth: 260,
  border: "1px solid var(--line)",
  borderRadius: 6,
  background: "var(--bg-2)",
  color: "var(--ink-0)",
  padding: "0 8px",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  outline: "none",
};

type DetailTab = "history" | "diff" | "targets" | "diagnose";

function SkillDetail({
  skill,
  targets,
  bindings,
  onMutation,
  onTrashed,
  readOnly,
}: {
  skill: Skill;
  targets: Target[];
  bindings: Binding[];
  onMutation: () => void;
  onTrashed: () => void;
  readOnly: boolean;
}) {
  const [tab, setTab] = useState<DetailTab>("history");
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyEvents, setHistoryEvents] = useState<LifecycleEvent[]>([]);
  const [historyRefreshKey, setHistoryRefreshKey] = useState(0);
  const [diagnoseLoading, setDiagnoseLoading] = useState(false);
  const [diagnoseError, setDiagnoseError] = useState<string | null>(null);
  const [diagnose, setDiagnose] = useState<SkillDiagnosePayload | null>(null);
  const [diagnoseRefreshKey, setDiagnoseRefreshKey] = useState(0);

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
  }, [skill.name, skill.latestRev, tab, historyRefreshKey]);

  useEffect(() => {
    if (tab !== "diagnose") return;
    const ctrl = new AbortController();
    setDiagnoseLoading(true);
    setDiagnoseError(null);
    setDiagnose(null);
    api
      .skillDiagnose(skill.name, ctrl.signal)
      .then((payload) => {
        if (ctrl.signal.aborted) return;
        setDiagnose(payload);
        setDiagnoseLoading(false);
      })
      .catch((err: Error) => {
        if (err.name !== "AbortError") {
          setDiagnoseError(err.message);
          setDiagnose(null);
          setDiagnoseLoading(false);
        }
      });
    return () => ctrl.abort();
  }, [skill.name, tab, diagnoseRefreshKey]);

  const onLifecycleMutation = () => {
    setHistoryRefreshKey((value) => value + 1);
    setDiagnoseRefreshKey((value) => value + 1);
    onMutation();
  };

  return (
    <div className="detail">
      <h4>{skill.name}</h4>
      {skill.description && <div style={detailDescriptionStyle}>{skill.description}</div>}
      <div className="dpath">skills/{skill.name}@{skill.latestRev}</div>
      <div className="kv">
        <div className="k">Source</div>
        <div className="v">{skill.sourceStatus}</div>
        <div className="k">Latest rev</div>
        <div className="v">{skill.latestRev}</div>
        <div className="k">Tags</div>
        <div className="v">{formatSkillTags(skill)}</div>
        <div className="k">Bindings</div>
        <div className="v">{skill.bindingCount}</div>
        <div className="k">Projections</div>
        <div className="v">{skill.projectionCount}</div>
        <div className="k">Policy</div>
        <div className="v">{policyLabel}</div>
      </div>

      <LifecycleActions skillName={skill.name} onMutation={onLifecycleMutation} readOnly={readOnly} />
      <TrashSkillAction skill={skill} onSuccess={onTrashed} readOnly={readOnly} />

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
        <button className={tab === "diagnose" ? "active" : ""} onClick={() => setTab("diagnose")}>
          Diagnose
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
      {tab === "diagnose" && (
        <SkillDiagnosePanel loading={diagnoseLoading} error={diagnoseError} diagnose={diagnose} />
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
      <MutationBanner state={project} spacing="top" />
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

const fullWidthButtonStyle: React.CSSProperties = {
  width: "100%",
  justifyContent: "center",
};

const skillDescriptionStyle: React.CSSProperties = {
  maxWidth: 420,
  marginTop: 3,
  color: "var(--ink-3)",
  fontSize: 11,
  fontWeight: 400,
  lineHeight: 1.35,
  whiteSpace: "normal",
};

const detailDescriptionStyle: React.CSSProperties = {
  margin: "4px 0 8px",
  color: "var(--ink-2)",
  fontSize: 12,
  lineHeight: 1.45,
};
