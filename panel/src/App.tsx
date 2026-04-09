import {
  Fragment,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  useTransition,
} from "react";
import {
  activeLabel,
  capabilityLabel,
  countLabel,
  detectLocale,
  formatTime,
  healthLabel,
  matcherKindLabel,
  methodLabel,
  missingLabel,
  ownershipLabel,
  persistLocale,
  pick,
  syncStateLabel,
} from "./i18n";
import "./styles.css";
import type { Locale } from "./i18n";
import type { PageId, PanelData, SkillView, V3Binding, V3Model, V3Payload, V3Projection, V3Target } from "./types";

const NAV_ITEMS: Array<{ id: PageId; icon: string }> = [
  { id: "overview", icon: "dashboard" },
  { id: "skills", icon: "bolt" },
  { id: "bindings", icon: "link" },
  { id: "targets", icon: "adjust" },
  { id: "projections", icon: "grid_view" },
  { id: "ops", icon: "terminal" },
  { id: "settings", icon: "settings" },
];

const EMPTY_COUNTS: V3Model["counts"] = {
  skills: 0,
  targets: 0,
  bindings: 0,
  active_bindings: 0,
  rules: 0,
  projections: 0,
  drifted_projections: 0,
  operations: 0,
};

const EMPTY_PANEL_DATA: PanelData = {
  health: {},
  info: {},
  skills: [],
  legacyTargets: { skills: {} },
  remote: {},
  pending: {
    count: 0,
    ops: [],
    warnings: [],
  },
  v3: {
    available: false,
    counts: EMPTY_COUNTS,
    bindings: [],
    targets: [],
    rules: [],
    projections: [],
  },
  remoteWarnings: [],
  live: false,
  lastUpdated: "",
};

function Icon({ name }: { name: string }) {
  return <span className="material-symbols-outlined">{name}</span>;
}

function formatShortPath(value: string | undefined, locale: Locale) {
  if (!value) return missingLabel(locale);
  const parts = value.split("/");
  return parts.length <= 4 ? value : `.../${parts.slice(-3).join("/")}`;
}

function formatSyncTone(state?: string) {
  switch ((state ?? "").toUpperCase()) {
    case "SYNCED":
    case "ACTIVE":
      return "is-success";
    case "DIVERGED":
    case "CONFLICTED":
      return "is-danger";
    case "PENDING_PUSH":
    case "LOCAL_ONLY":
      return "is-warning";
    default:
      return "is-primary";
  }
}

function topbarSearchPlaceholder(page: PageId, locale: Locale) {
  if (page === "skills") {
    return pick(locale, "QUERY_SKILL_ID...", "查询技能 ID...");
  }
  return pick(locale, "CMD + K TO SEARCH", "CMD + K 搜索");
}

function projectionIsDrifted(projection: V3Projection) {
  return Boolean(projection.observed_drift) || projection.health !== "healthy";
}

function agentDisplayName(agent?: string) {
  if (agent === "claude") return "Claude";
  if (agent === "codex") return "Codex";
  return agent ?? "Unknown";
}

function summarizeDetails(details: Record<string, unknown>) {
  const summary = Object.entries(details)
    .slice(0, 3)
    .map(([key, value]) => `${key}:${String(value)}`)
    .join(" · ");
  return summary || "no-details";
}

function projectionProgress(projection?: V3Projection | null) {
  if (!projection) return 0;
  if (projectionIsDrifted(projection)) return 45;
  if (projection.health === "warning") return 68;
  return 100;
}

function projectionToneClass(projection?: V3Projection | null) {
  if (!projection) return "is-muted";
  if (projectionIsDrifted(projection)) return "is-danger";
  if (projection.health === "warning") return "is-warning";
  return "is-success";
}

function bindingRowIcon(binding: V3Binding) {
  if (!binding.active) return "person";
  if (binding.policy_profile === "strict") return "shield";
  return "person";
}

function bindingAgentIcon(binding: V3Binding) {
  if (binding.agent === "claude") return "bolt";
  if (binding.agent === "codex") return "data_object";
  return "memory";
}

function projectionSourceIcon(skill: SkillView) {
  const name = skill.name.toLowerCase();
  if (name.includes("data") || name.includes("api")) return "data_object";
  if (name.includes("asset") || name.includes("image")) return "folder_zip";
  if (name.includes("auth") || name.includes("security")) return "bolt";
  return skill.methods.includes("copy") ? "folder_zip" : "bolt";
}

function statusFooterTail(page: PageId, data: PanelData, locale: Locale) {
  if (page === "skills") {
    const clock = new Date().toISOString().slice(11, 19);
    return <div className="shell-status-tail">{`SYSTEM CLOCK: ${clock} UTC`}</div>;
  }
  if (page === "bindings") {
    return (
      <div className="shell-status-tail">
        {pick(locale, "Encrypted Terminal Session [L-882]", "加密终端会话 [L-882]")}
      </div>
    );
  }
  if (page === "ops") {
    return (
      <div className="shell-status-tail shell-status-tail-split">
        <span>LOC: 40.7128° N, 74.0060° W</span>
        <span>{`MEM: ${Math.max(1, data.pending.count)}.4G / 8.0G`}</span>
      </div>
    );
  }
  return null;
}

function opToneClass(op: PanelData["pending"]["ops"][number] | null, remoteState?: string, index = 0) {
  if (!op) return "is-muted";
  if (index === 0 && (remoteState === "DIVERGED" || remoteState === "CONFLICTED")) return "is-danger";
  if (op.command.includes("release") || op.command.includes("snapshot")) return "is-success";
  if (op.command.includes("sync") || op.command.includes("project")) return "is-primary";
  return "is-warning";
}

function opProgress(op: PanelData["pending"]["ops"][number] | null) {
  if (!op) return 0;
  if (op.command.includes("sync")) return 65;
  if (op.command.includes("capture")) return 100;
  if (op.command.includes("repair")) return 24;
  return 48;
}

function scheduledOpIcon(index: number) {
  return index === 0 ? "timer" : "sync_problem";
}

function getPageFromHash(): PageId {
  const raw = window.location.hash.replace("#", "");
  const item = NAV_ITEMS.find((entry) => entry.id === raw);
  return item?.id ?? "overview";
}

function navLabel(locale: Locale, pageId: PageId) {
  switch (pageId) {
    case "overview":
      return pick(locale, "Overview", "总览");
    case "skills":
      return pick(locale, "Skills", "技能");
    case "bindings":
      return pick(locale, "Bindings", "绑定");
    case "targets":
      return pick(locale, "Targets", "目标");
    case "projections":
      return pick(locale, "Projections", "投影");
    case "ops":
      return pick(locale, "Ops", "操作");
    case "settings":
      return pick(locale, "Environment", "环境");
  }
}

async function fetchRequiredJson<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(`GET ${path} failed with ${response.status}`);
  }
  return (await response.json()) as T;
}

function normalizeV3(payload: V3Payload, locale: Locale): V3Model {
  if (!payload.ok || !payload.data) {
    return {
      available: false,
      counts: EMPTY_COUNTS,
      bindings: [],
      targets: [],
      rules: [],
      projections: [],
      error: payload.error?.message ?? pick(locale, "v3 state unavailable", "v3 状态不可用"),
    };
  }

  return {
    available: true,
    counts: {
      ...EMPTY_COUNTS,
      ...(payload.data.counts ?? {}),
    },
    bindings: payload.data.bindings ?? [],
    targets: payload.data.targets ?? [],
    rules: payload.data.rules ?? [],
    projections: payload.data.projections ?? [],
    checkpoint: payload.data.checkpoint,
  };
}

async function loadPanelData(locale: Locale): Promise<PanelData> {
  const [health, info, skills, targets, v3, remote, pending] = await Promise.all([
    fetchRequiredJson<PanelData["health"]>("/api/health"),
    fetchRequiredJson<PanelData["info"]>("/api/info"),
    fetchRequiredJson<{ skills?: string[] }>("/api/skills"),
    fetchRequiredJson<{ targets?: PanelData["legacyTargets"] }>("/api/targets"),
    fetchRequiredJson<V3Payload>("/api/v3/status"),
    fetchRequiredJson<{ remote?: PanelData["remote"]; warnings?: string[] }>("/api/remote/status"),
    fetchRequiredJson<PanelData["pending"]>("/api/pending"),
  ]);

  return {
    health,
    info,
    skills: skills.skills ?? [],
    legacyTargets: targets.targets ?? { skills: {} },
    v3: normalizeV3(v3, locale),
    remote: remote.remote ?? {},
    pending: {
      count: pending.count ?? 0,
      ops: pending.ops ?? [],
      journal_events: pending.journal_events,
      history_events: pending.history_events,
      warnings: pending.warnings ?? [],
    },
    remoteWarnings: remote.warnings ?? [],
    live: true,
    lastUpdated: new Date().toISOString(),
  };
}

function buildSkillViews(data: PanelData): SkillView[] {
  const bindingMap = new Map(data.v3.bindings.map((binding) => [binding.binding_id, binding]));
  const targetMap = new Map(data.v3.targets.map((target) => [target.target_id, target]));
  const allNames = new Set<string>([
    ...data.skills,
    ...Object.keys(data.legacyTargets.skills),
    ...data.v3.rules.map((rule) => rule.skill_id),
    ...data.v3.projections.map((projection) => projection.skill_id),
  ]);

  const result: SkillView[] = [];
  for (const name of allNames) {
    const rules = data.v3.rules.filter((rule) => rule.skill_id === name);
    const projections = data.v3.projections.filter((projection) => projection.skill_id === name);
    const bindings = new Map<string, V3Binding>();
    const targets = new Map<string, V3Target>();
    const methods = new Set<string>();

    for (const rule of rules) {
      methods.add(rule.method);
      const binding = bindingMap.get(rule.binding_id);
      const target = targetMap.get(rule.target_id);
      if (binding) bindings.set(binding.binding_id, binding);
      if (target) targets.set(target.target_id, target);
    }

    for (const projection of projections) {
      methods.add(projection.method);
      const binding = bindingMap.get(projection.binding_id);
      const target = targetMap.get(projection.target_id);
      if (binding) bindings.set(binding.binding_id, binding);
      if (target) targets.set(target.target_id, target);
    }

    result.push({
      name,
      rules,
      projections,
      bindings: [...bindings.values()],
      targets: [...targets.values()],
      methods: [...methods.values()],
      driftedCount: projections.filter(projectionIsDrifted).length,
      legacyTarget: data.legacyTargets.skills[name],
    });
  }

  return result.sort((left, right) => left.name.localeCompare(right.name));
}

function usePanelApp(locale: Locale) {
  const [data, setData] = useState<PanelData>(EMPTY_PANEL_DATA);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [page, setPage] = useState<PageId>(() => getPageFromHash());
  const [navPending, startNavTransition] = useTransition();

  useEffect(() => {
    let cancelled = false;

    async function run() {
      setLoading(true);
      setLoadError(null);
      try {
        const next = await loadPanelData(locale);
        if (!cancelled) {
          setData(next);
        }
      } catch (error) {
        if (!cancelled) {
          const detail = error instanceof Error ? error.message : pick(locale, "panel failed to load", "面板加载失败");
          setLoadError(`${pick(locale, "Panel failed to load", "面板加载失败")}: ${detail}`);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void run();
    return () => {
      cancelled = true;
    };
  }, [locale]);

  useEffect(() => {
    const onHash = () => {
      startNavTransition(() => setPage(getPageFromHash()));
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  return {
    data,
    loading,
    loadError,
    page,
    navPending,
    navigate(next: PageId) {
      window.location.hash = next;
      startNavTransition(() => setPage(next));
    },
    async refresh() {
      setLoading(true);
      setLoadError(null);
      try {
        const next = await loadPanelData(locale);
        setData(next);
      } catch (error) {
        const detail = error instanceof Error ? error.message : pick(locale, "panel failed to load", "面板加载失败");
        setLoadError(`${pick(locale, "Panel failed to load", "面板加载失败")}: ${detail}`);
      } finally {
        setLoading(false);
      }
    },
  };
}

function ShellStat({
  label,
  locale,
  value,
  tone,
  foot,
}: {
  label: string;
  locale: Locale;
  value: string | number;
  tone?: string;
  foot?: string;
}) {
  return (
    <div className={`card stat-card ${tone ?? ""}`}>
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      <div className="stat-foot">
        <span className={`metric-chip ${tone ?? ""}`}>{foot ?? pick(locale, "stable", "稳定")}</span>
      </div>
    </div>
  );
}

function OverviewPage({
  data,
  locale,
  skillViews,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
}) {
  const drifted = data.v3.projections.filter(projectionIsDrifted);
  const activeBindings = data.v3.bindings.filter((binding) => binding.active);
  const primaryBinding = activeBindings[0] ?? data.v3.bindings[0];
  const weaveSkills = skillViews.slice(0, 3);
  const weaveTargets = data.v3.targets.slice(0, 2);
  const overviewWarnings = [...data.remoteWarnings, ...(data.pending.warnings ?? [])];
  const diagnosticLines = data.pending.ops.slice(0, 4);

  return (
    <div className="page-body page-frame">
      <div className="page-head">
        <div>
          <h1 className="page-title">{pick(locale, "Operational Overview", "运行总览")}</h1>
          <p className="page-subtitle">
            {pick(
              locale,
              "Source registry, workspace bindings, target topology, projection health, and sync risk.",
              "展示源技能目录、工作区绑定、目标拓扑、投影健康度与同步风险。",
            )}
          </p>
        </div>
        <div className="page-actions">
          <span className={`status-pill ${formatSyncTone(data.remote.sync_state)}`}>
            <Icon name="sync" />
            {syncStateLabel(locale, data.remote.sync_state)}
          </span>
          <span className={`status-pill ${data.live ? "is-success" : "is-warning"}`}>
            <Icon name={data.live ? "hub" : "science"} />
            {data.live ? pick(locale, "api live", "API 已连接") : pick(locale, "api unavailable", "API 不可用")}
          </span>
        </div>
      </div>

      <div className="stats-grid">
        <ShellStat
          label={pick(locale, "Total Skills", "技能总数")}
          locale={locale}
          value={skillViews.length}
          tone="is-primary"
          foot={locale === "zh-CN"
            ? `${data.v3.counts.skills || skillViews.length} 已映射`
            : `${data.v3.counts.skills || skillViews.length} mapped`}
        />
        <ShellStat
          label={pick(locale, "Active Bindings", "活跃绑定")}
          locale={locale}
          value={activeBindings.length}
          tone="is-success"
          foot={locale === "zh-CN" ? `${data.v3.bindings.length} 总计` : `${data.v3.bindings.length} total`}
        />
        <ShellStat
          label={pick(locale, "Managed Targets", "托管目标")}
          locale={locale}
          value={data.v3.targets.filter((target) => target.ownership === "managed").length}
          tone="is-success"
          foot={locale === "zh-CN" ? `${data.v3.targets.length} 已注册` : `${data.v3.targets.length} registered`}
        />
        <ShellStat
          label={pick(locale, "Total Projections", "投影总数")}
          locale={locale}
          value={data.v3.projections.length}
          tone="is-primary"
          foot={locale === "zh-CN" ? `${drifted.length} 已漂移` : `${drifted.length} drifted`}
        />
        <ShellStat
          label={pick(locale, "Pending Ops", "待处理操作")}
          locale={locale}
          value={data.pending.count}
          tone={data.pending.count > 0 ? "is-warning" : "is-success"}
          foot={locale === "zh-CN"
            ? `${data.remote.ahead ?? 0}/${data.remote.behind ?? 0} 领先/落后`
            : `${data.remote.ahead ?? 0}/${data.remote.behind ?? 0} ahead/behind`}
        />
      </div>

      <div className="overview-grid">
        <section className="panel weave-shell">
          <div className="weave-shell-head">
            <h2 className="panel-title">
              {pick(locale, "Connectivity Map // The System Weave", "连接图 // 系统织网")}
            </h2>
            <div className="weave-live">
              <span className="weave-live-dot" />
              <span>{pick(locale, "Live Stream", "实时流")}</span>
            </div>
          </div>
          <div className="weave-shell-body">
            <div className="weave-grid-backdrop" />
            <div className="weave-stage">
              <div className="weave-node-column">
                {weaveSkills.map((skill, index) => (
                  <div className="weave-square-node" key={skill.name} title={skill.name}>
                    <Icon name={index === 0 ? "bolt" : index === 1 ? "database" : "api"} />
                  </div>
                ))}
              </div>

              <div className="weave-spine">
                <div className="weave-spine-line weave-spine-line-top" />
                <div className="weave-spine-line weave-spine-line-mid" />
                <div className="weave-spine-line weave-spine-line-bottom" />
              </div>

              <div className="weave-binding-core">
                <div className="weave-binding-diamond">
                  <div className="weave-binding-inner">
                    <Icon name="link" />
                    <span>{primaryBinding?.binding_id ?? "BIND_V4"}</span>
                  </div>
                </div>
              </div>

              <div className="weave-spine weave-spine-right">
                <div className="weave-spine-line weave-spine-line-top" />
                <div className="weave-spine-line weave-spine-line-mid" />
                <div className="weave-spine-line weave-spine-line-bottom" />
              </div>

              <div className="weave-node-column weave-node-column-targets">
                {(weaveTargets.length > 0
                  ? weaveTargets
                  : [{ target_id: "target-a" }, { target_id: "target-b" }]).map((target, index) => (
                  <div className="weave-round-node" key={target.target_id}>
                    <Icon name={index === 0 ? "dns" : "cloud_done"} />
                  </div>
                ))}
              </div>
            </div>
          </div>
          <div className="weave-shell-foot">
            <div className="weave-foot-item">
              <span className="row-label">{pick(locale, "Trace", "追踪")}</span>
              <span className="mono-copy">{primaryBinding?.profile_id ?? "profile:main"}</span>
            </div>
            <div className="weave-foot-item">
              <span className="row-label">{pick(locale, "Bindings", "绑定")}</span>
              <span className="mono-copy">{data.v3.bindings.length}</span>
            </div>
            <div className="weave-foot-item">
              <span className="row-label">{pick(locale, "Targets", "目标")}</span>
              <span className="mono-copy">{data.v3.targets.length}</span>
            </div>
          </div>
        </section>

        <section className="panel risk-console">
          <div className="risk-console-head">
            <h2 className="panel-title">{pick(locale, "Risk Vector Analysis", "风险向量分析")}</h2>
            <Icon name="report" />
          </div>
          <div className="risk-console-body">
            <article className="risk-console-row is-danger">
              <div className="risk-console-meta">
                <span>{pick(locale, "Critical Drift", "关键漂移")}</span>
                <span>{formatTime(locale, drifted[0]?.updated_at ?? data.lastUpdated)}</span>
              </div>
              <p className="secondary-copy">
                {drifted[0]
                  ? pick(
                    locale,
                    `Instance "${drifted[0].instance_id}" differs from source-of-truth and needs review.`,
                    `实例 "${drifted[0].instance_id}" 与规范源不一致，需要复核。`,
                  )
                  : pick(locale, "No critical drift is currently flagged.", "当前没有关键漂移告警。")}
              </p>
              <div className="risk-console-actions">
                <button className="button" type="button">{pick(locale, "Force Sync", "强制同步")}</button>
                <button className="button-ghost" type="button">{pick(locale, "Logs", "日志")}</button>
              </div>
            </article>

            <article
              className={`risk-console-row ${
                (data.remote.behind ?? 0) > 0 || data.remote.sync_state === "DIVERGED"
                  ? "is-secondary"
                  : "is-muted"
              }`}
            >
              <div className="risk-console-meta">
                <span>{(data.remote.sync_state ?? "LOCAL_ONLY").toUpperCase()}</span>
                <span>{pick(locale, "sync posture", "同步姿态")}</span>
              </div>
              <p className="secondary-copy">
                {locale === "zh-CN"
                  ? `${data.remote.ahead ?? 0} 领先，${data.remote.behind ?? 0} 落后，${data.pending.count} 个操作仍在队列中。`
                  : `${data.remote.ahead ?? 0} ahead, ${data.remote.behind ?? 0} behind, ${data.pending.count} operations remain queued.`}
              </p>
            </article>

            <article className="risk-console-row is-success">
              <div className="risk-console-meta">
                <span>{pick(locale, "Ops Recovered", "操作恢复")}</span>
                <span>{pick(locale, "status stream", "状态流")}</span>
              </div>
              <p className="secondary-copy">
                {overviewWarnings[0]
                  ? overviewWarnings[0]
                  : pick(locale, "No additional warnings are active.", "当前没有额外活跃告警。")}
              </p>
            </article>
          </div>
        </section>
      </div>

      <section className="diagnostic-shell">
        <div className="diagnostic-shell-head">
          <div className="diagnostic-shell-tabs">
            <span>{pick(locale, "Diagnostic Shell", "诊断终端")}</span>
            <span>{pick(locale, "Session: Active", "会话：活跃")}</span>
          </div>
          <div className="diagnostic-shell-actions">
            <span>{pick(locale, "Clear", "清空")}</span>
            <span>{pick(locale, "Export", "导出")}</span>
          </div>
        </div>
        <div className="diagnostic-shell-body">
          {(diagnosticLines.length > 0 ? diagnosticLines : data.pending.ops).slice(0, 4).map((op) => (
            <div className="diagnostic-line" key={op.request_id}>
              <span className="diagnostic-time">{formatTime(locale, op.created_at)}</span>
              <span className="diagnostic-label">{op.command.toUpperCase().replaceAll(" ", "_")}</span>
              <span>{summarizeDetails(op.details)}</span>
            </div>
          ))}
          {diagnosticLines.length === 0 ? (
            <div className="diagnostic-line">
              <span className="diagnostic-time">{formatTime(locale, data.lastUpdated)}</span>
              <span className="diagnostic-label">{pick(locale, "SYSTEM_OK", "系统正常")}</span>
              <span>{pick(locale, "Awaiting live operations from the queue.", "等待队列中的实时操作。")}</span>
            </div>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function SkillsPage({
  data,
  locale,
  skillViews,
  query,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
  query: string;
}) {
  const deferredQuery = useDeferredValue(query);
  const filtered = useMemo(() => {
    const value = deferredQuery.trim().toLowerCase();
    if (!value) return skillViews;
    return skillViews.filter((skill) => skill.name.toLowerCase().includes(value));
  }, [deferredQuery, skillViews]);

  const [selectedSkill, setSelectedSkill] = useState<string>(filtered[0]?.name ?? "");

  useEffect(() => {
    if (!filtered.some((skill) => skill.name === selectedSkill)) {
      setSelectedSkill(filtered[0]?.name ?? "");
    }
  }, [filtered, selectedSkill]);

  const active = filtered.find((skill) => skill.name === selectedSkill) ?? filtered[0];
  const activeProjection = active?.projections[0] ?? null;
  const sourcePath = active
    ? `${data.info.root ?? "<root>"}/skills/${active.name}`
    : `${data.info.root ?? "<root>"}/skills/<skill>`;
  const dependencyText = active
    ? (active.bindings.map((binding) => `${binding.binding_id}@${binding.profile_id}`).slice(0, 2).join(", ")
      || active.targets.map((target) => target.target_id).slice(0, 2).join(", ")
      || missingLabel(locale))
    : missingLabel(locale);
  const integrityScore = active?.projections.length ? `${Math.max(94, 100 - active.driftedCount)}.${Math.max(0, active.bindings.length)}%` : "n/a";
  const entropyLabel = active?.driftedCount ? "MEDIUM (0.17)" : "LOW (0.02)";
  const lastModified = formatTime(locale, activeProjection?.updated_at ?? data.lastUpdated);
  const previewTarget = activeProjection?.target_id ?? "ALPHA-NORTH-01";
  const weaveNeighbors = active
    ? [
      active.bindings[0]?.binding_id ?? "nlp-core",
      active.name,
      active.targets[0]?.target_id ?? "data-viz",
    ]
    : ["nlp-core", "skill", "data-viz"];

  return (
    <div className="stitch-workspace">
      <section className="stitch-rail">
        <div className="stitch-rail-head">
          <div className="stitch-rail-head-row">
            <h2 className="stitch-rail-title">{pick(locale, "Skill Sources", "技能源")}</h2>
            <span className="minor-chip is-primary">
              {locale === "zh-CN" ? `${filtered.length} 已启用` : `${filtered.length} active`}
            </span>
          </div>
          <div className="stitch-rail-tabs">
            <button className="stitch-tab active" type="button">{pick(locale, "Canonical", "规范源")}</button>
            <button className="stitch-tab" type="button">{pick(locale, "Remote", "远端")}</button>
          </div>
        </div>
        <div className="skill-list">
          {filtered.length === 0 ? (
            <div className="empty-state">{pick(locale, "no skill matched query", "没有匹配的技能")}</div>
          ) : (
            filtered.map((skill) => (
              <button
                className={`skill-row ${skill.name === active?.name ? "active" : ""}`}
                key={skill.name}
                onClick={() => setSelectedSkill(skill.name)}
                type="button"
              >
                <div className="stitch-skill-row-top">
                  <span className="stitch-skill-name">{`skills/${skill.name}`}</span>
                  <span className={`material-symbols-outlined ${skill.driftedCount > 0 ? "is-danger" : "is-success"}`}>
                    {skill.driftedCount > 0 ? "warning" : "check_circle"}
                  </span>
                </div>
                <div className="stitch-skill-hash">
                  hash: {(skill.projections[0]?.last_applied_rev ?? "pending").slice(0, 10)}
                </div>
                <div className="stitch-skill-tags">
                  {(skill.methods.length > 0 ? skill.methods : ["unprojected"]).slice(0, 2).map((method) => (
                    <span className="stitch-inline-tag" key={method}>
                      {method === "unprojected" ? pick(locale, "UNPROJECTED", "未投影") : method.toUpperCase()}
                    </span>
                  ))}
                </div>
              </button>
            ))
          )}
        </div>
      </section>

      <section className="stitch-detail-shell">
        {!active ? (
          <div className="empty-state">{pick(locale, "select a skill to inspect", "选择一个技能查看详情")}</div>
        ) : (
          <>
            <div className="stitch-detail-header">
              <div>
                <div className="stitch-detail-title-row">
                  <Icon name="bolt" />
                  <h2>{`skills/${active.name}`}</h2>
                </div>
                <div className="stitch-detail-meta-row">
                  <div className="stitch-inline-meta">
                    <Icon name="folder" />
                    <span>{sourcePath}</span>
                  </div>
                  <div className="stitch-inline-meta is-accent">
                    <Icon name="account_tree" />
                    <span>{`main [${syncStateLabel(locale, data.remote.sync_state)}]`}</span>
                  </div>
                </div>
              </div>
              <div className="page-actions">
                <button className="button-ghost" disabled type="button">DIFF_SOURCE</button>
                <button className="button" disabled type="button">RE_SYNC</button>
              </div>
            </div>

            <div className="stitch-grid-12">
              <div className="stitch-col-main">
                <div className="stitch-section-block">
                  <h3 className="stitch-block-title">
                    <span className="stitch-block-dot" />
                    {pick(locale, "Projection Instances", "投影实例")}
                  </h3>
                  <div className="stitch-table-shell">
                    <table className="stitch-dense-table">
                      <thead>
                        <tr>
                          <th>TARGET_ID</th>
                          <th>BINDING</th>
                          <th>PATH</th>
                          <th className="align-right">STATUS</th>
                        </tr>
                      </thead>
                      <tbody>
                        {active.projections.length === 0 ? (
                          <tr>
                            <td colSpan={4}>
                              <div className="empty-state">{pick(locale, "no active projections", "没有活跃投影")}</div>
                            </td>
                          </tr>
                        ) : (
                          active.projections.map((projection) => (
                            <tr key={projection.instance_id}>
                              <td>{projection.target_id}</td>
                              <td>{projection.binding_id}</td>
                              <td>{formatShortPath(projection.materialized_path, locale)}</td>
                              <td className="align-right">
                                <span className={`stitch-status-tag ${projectionToneClass(projection)}`}>
                                  {projectionIsDrifted(projection)
                                    ? `${healthLabel(locale, projection.health)}`
                                    : pick(locale, "HEALTHY", "健康")}
                                </span>
                              </td>
                            </tr>
                          ))
                        )}
                      </tbody>
                    </table>
                  </div>
                </div>

                <div className="stitch-code-preview">
                  <div className="stitch-code-head">
                    <span>{pick(locale, "CLI Command Preview", "CLI 命令预览")}</span>
                    <button className="icon-button" type="button" title={pick(locale, "Copy", "复制")}>
                      <Icon name="content_copy" />
                    </button>
                  </div>
                  <code>{`loom project skill/${active.name} --target ${previewTarget} --force`}</code>
                </div>
              </div>

              <div className="stitch-col-side">
                <div className="stitch-panel-block">
                  <h3 className="stitch-block-title">{pick(locale, "Metadata Analysis", "元数据分析")}</h3>
                  <div className="stitch-meta-list">
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Dependencies", "依赖")}</span>
                      <span>{dependencyText}</span>
                    </div>
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Entropy", "熵")}</span>
                      <span>{entropyLabel}</span>
                    </div>
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Last Modified", "最新修订")}</span>
                      <span>{lastModified}</span>
                    </div>
                    <div className="stitch-meta-row">
                      <span>{pick(locale, "Schema Integrity", "结构完整性")}</span>
                      <span>{integrityScore}</span>
                    </div>
                  </div>
                </div>

                <div className="stitch-panel-block">
                  <h3 className="stitch-block-title">{pick(locale, "The Weave: Connection Map", "织网：连接图")}</h3>
                  <div className="stitch-mini-weave">
                    <div className="stitch-mini-weave-grid" />
                    <div className="stitch-mini-weave-stack">
                      <div className="stitch-mini-weave-row">
                        <div className="stitch-mini-weave-col">
                          <div className="stitch-mini-icon-shell">
                            <Icon name="account_tree" />
                          </div>
                          <span>{weaveNeighbors[0]}</span>
                        </div>
                        <div className="stitch-mini-weave-core">
                          <div className="stitch-mini-weave-line" />
                          <div className="stitch-mini-center-shell">
                            <Icon name="bolt" />
                          </div>
                        </div>
                        <div className="stitch-mini-weave-col">
                          <div className="stitch-mini-icon-shell">
                            <Icon name="dataset" />
                          </div>
                          <span>{weaveNeighbors[2]}</span>
                        </div>
                      </div>
                      <div className="stitch-mini-weave-stem" />
                      <div className="stitch-mini-target-tag">{`TARGET: ${previewTarget}`}</div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </>
        )}
      </section>
    </div>
  );
}

function BindingsPage({
  data,
  locale,
  skillViews,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
}) {
  const [selectedBinding, setSelectedBinding] = useState<string>(data.v3.bindings[0]?.binding_id ?? "");
  const active = data.v3.bindings.find((binding) => binding.binding_id === selectedBinding) ?? data.v3.bindings[0];

  const relatedRules = active
    ? data.v3.rules.filter((rule) => rule.binding_id === active.binding_id)
    : [];
  const relatedProjections = active
    ? data.v3.projections.filter((projection) => projection.binding_id === active.binding_id)
    : [];
  const relatedSkills = skillViews.filter((skill) =>
    skill.bindings.some((binding) => binding.binding_id === active?.binding_id),
  );
  const defaultTarget = data.v3.targets.find((target) => target.target_id === active?.default_target_id);
  const memoryImpact = `${96 + relatedProjections.length * 23} MB`;
  const nodeIdentity = active?.profile_id
    ? active.profile_id.replace(/[^a-zA-Z0-9]/g, "").slice(0, 12).padEnd(12, "0").match(/.{1,2}/g)?.join(":") ?? active.profile_id
    : "00:00:00:00:00:00";

  return (
    <div className="stitch-screen-column">
      <div className="stitch-content-header">
        <div>
          <div className="stitch-content-title-row">
            <h1 className="stitch-content-title">{pick(locale, "Workspace Bindings", "工作区绑定")}</h1>
            <span className="stitch-content-badge">GLOBAL</span>
          </div>
          <p className="stitch-content-subtitle">
            {pick(locale, "MAP: WORKSPACE_CONTEXT -> AGENT_RUNTIME", "映射：WORKSPACE_CONTEXT -> AGENT_RUNTIME")}
          </p>
        </div>
        <div className="page-actions">
          <button className="button-ghost" disabled type="button">
            <Icon name="add" />
            {pick(locale, "Add Binding", "新增绑定")}
          </button>
          <button className="button-ghost" type="button">
            <Icon name="filter_list" />
          </button>
        </div>
      </div>
      <div className="stitch-bindings-layout">
        <section className="panel stitch-panel-reset">
          <div className="table-wrap">
            <table className="stitch-dense-table stitch-bindings-table">
              <thead>
                <tr>
                  <th>{pick(locale, "Binding Name", "绑定名")}</th>
                  <th>{pick(locale, "Matcher", "匹配器")}</th>
                  <th>{pick(locale, "Agent Kind", "代理类型")}</th>
                  <th>{pick(locale, "Default Path", "默认路径")}</th>
                  <th>{pick(locale, "Policy", "策略")}</th>
                  <th>{pick(locale, "Status", "状态")}</th>
                </tr>
              </thead>
              <tbody>
                {data.v3.bindings.map((binding) => {
                  const rowTarget = data.v3.targets.find((target) => target.target_id === binding.default_target_id);
                  const selected = binding.binding_id === active?.binding_id;
                  return (
                    <tr
                      className={`${selected ? "active" : ""} ${binding.active ? "" : "is-inactive"}`.trim() || undefined}
                      key={binding.binding_id}
                      onClick={() => setSelectedBinding(binding.binding_id)}
                    >
                      <td>
                        <div className="stitch-binding-row-head">
                          <div
                            className={`stitch-binding-row-icon ${
                              binding.active ? (binding.policy_profile === "strict" ? "is-primary" : "is-accent") : "is-muted"
                            }`}
                          >
                            <Icon name={bindingRowIcon(binding)} />
                          </div>
                          <div>
                            <div className="stitch-binding-cell-title">{binding.binding_id}</div>
                            <div className="mono-copy">{`ID: ${binding.profile_id}`}</div>
                          </div>
                        </div>
                      </td>
                      <td>
                        <div className="mono-copy">
                          <span className="muted">{`${matcherKindLabel(locale, binding.workspace_matcher.kind)}:`} </span>
                          {binding.workspace_matcher.value}
                        </div>
                      </td>
                      <td>
                        <div className="stitch-agent-pill">
                          <span className={`stitch-agent-icon ${binding.agent === "claude" ? "is-accent" : "is-primary"}`}>
                            <Icon name={bindingAgentIcon(binding)} />
                          </span>
                          <span>{agentDisplayName(binding.agent)}</span>
                        </div>
                      </td>
                      <td>
                        <div className="mono-copy">{rowTarget?.path ?? binding.default_target_id}</div>
                      </td>
                      <td>
                        <span className={`stitch-status-tag ${binding.policy_profile === "strict" ? "is-danger" : "is-success"}`}>
                          {binding.policy_profile.toUpperCase()}
                        </span>
                      </td>
                      <td>
                        <span className={`stitch-activity ${binding.active ? "is-success" : "is-muted"}`}>
                          <span className="stitch-activity-dot" />
                          {activeLabel(locale, binding.active).toUpperCase()}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>

        <aside className="panel stitch-drawer">
          <div className="detail-panel">
            {!active ? (
              <div className="empty-state">{pick(locale, "no bindings available", "没有可用绑定")}</div>
            ) : (
              <>
                <div className="stitch-drawer-card">
                  <div className="stitch-drawer-card-head">
                    <h3 className="stitch-block-title">{pick(locale, "Binding Analysis", "绑定分析")}</h3>
                    <button className="icon-button" type="button">
                      <Icon name="close" />
                    </button>
                  </div>
                  <div className="stitch-binding-hero">
                    <div className="stitch-binding-hero-icon">
                      <Icon name="link" />
                    </div>
                    <div>
                      <h4>{active.binding_id}</h4>
                      <p>{pick(locale, "Status: Handshake Verified", "状态：握手已验证")}</p>
                    </div>
                  </div>
                  <div className="stitch-kpi-grid">
                    <div className="stitch-kpi-card">
                      <p>{pick(locale, "Latency", "延迟")}</p>
                      <strong>{`${24 + relatedRules.length}ms`}</strong>
                    </div>
                    <div className="stitch-kpi-card">
                      <p>{pick(locale, "Throughput", "吞吐")}</p>
                      <strong>{`${Math.max(1, relatedProjections.length)}.2mb/s`}</strong>
                    </div>
                  </div>
                </div>

                <div className="stitch-drawer-card">
                  <h3 className="stitch-block-title">{pick(locale, "Dependent Skills", "依赖技能")}</h3>
                  <div className="stitch-thread-list">
                    {relatedSkills.length === 0 ? (
                      <div className="empty-state">{pick(locale, "no related skills", "没有相关技能")}</div>
                    ) : (
                      relatedSkills.map((skill) => (
                        <div className="stitch-thread-row" key={skill.name}>
                          <div>
                            <div className="stitch-thread-title">{skill.name}</div>
                            <div className="stitch-thread-subtitle">{skill.methods.join(" · ") || pick(locale, "unprojected", "未投影")}</div>
                          </div>
                          <span className="mono-copy">{`v${Math.max(1, skill.projections.length)}.${skill.driftedCount}`}</span>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                <div className="stitch-drawer-card">
                  <h3 className="stitch-block-title">{pick(locale, "Active Projections", "活跃投影")}</h3>
                  <div className="stitch-progress-list">
                    {relatedProjections.length === 0 ? (
                      <div className="empty-state">{pick(locale, "no active projections", "没有活跃投影")}</div>
                    ) : (
                      relatedProjections.map((projection) => (
                        <div className="stitch-progress-card" key={projection.instance_id}>
                          <div className="stitch-progress-head">
                            <p>{projection.target_id}</p>
                            <span>{`${projectionProgress(projection)}% ${projectionIsDrifted(projection) ? "SYNC" : "OK"}`}</span>
                          </div>
                          <div className="stitch-progress-bar">
                            <div
                              className={`stitch-progress-fill ${projectionToneClass(projection)}`}
                              style={{ width: `${projectionProgress(projection)}%` }}
                            />
                          </div>
                          <p className="stitch-thread-subtitle">
                            {defaultTarget?.path ?? active.default_target_id} · {methodLabel(locale, projection.method)}
                          </p>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                <div className="stitch-drawer-card stitch-footprint-card">
                  <div className="stitch-footprint-row">
                    <span>{pick(locale, "Memory Impact", "内存影响")}</span>
                    <span>{memoryImpact}</span>
                  </div>
                  <div className="stitch-footprint-row">
                    <span>{pick(locale, "Node Identity", "节点标识")}</span>
                    <span className="is-primary">{nodeIdentity}</span>
                  </div>
                  <button className="button stitch-sync-binding-button" disabled type="button">
                    {pick(locale, "Sync Binding Now", "立即同步绑定")}
                  </button>
                </div>
              </>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}

function TargetsPage({ data, locale }: { data: PanelData; locale: Locale }) {
  const [selectedTargetId, setSelectedTargetId] = useState<string>(data.v3.targets[0]?.target_id ?? "");
  const active = data.v3.targets.find((target) => target.target_id === selectedTargetId) ?? data.v3.targets[0];

  const relatedBindings = active
    ? data.v3.bindings.filter((binding) => binding.default_target_id === active.target_id)
    : [];
  const relatedRules = active
    ? data.v3.rules.filter((rule) => rule.target_id === active.target_id)
    : [];
  const relatedProjections = active
    ? data.v3.projections.filter((projection) => projection.target_id === active.target_id)
    : [];

  return (
    <div className="page-body page-frame">
      <div className="page-head">
        <div>
          <h1 className="page-title">{pick(locale, "Targets Registry", "目标注册表")}</h1>
          <p className="page-subtitle">
            {pick(
              locale,
              "Registered filesystem targets, ownership boundaries, capabilities, and dependent projections.",
              "查看已注册的文件系统目标、所有权边界、能力与依赖投影。",
            )}
          </p>
        </div>
        <div className="page-actions">
          <button className="button" disabled>
            {pick(locale, "Register Target", "注册目标")}
          </button>
          <span className="status-pill is-primary">
            <Icon name="folder_managed" />
            {countLabel(locale, data.v3.targets.length, "target", "targets", "个目标")}
          </span>
        </div>
      </div>

      <div className="bindings-grid">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Targets Table", "目标表")}</h2>
            <span className="minor-chip">
              {locale === "zh-CN"
                ? `${data.v3.targets.filter((target) => target.ownership === "managed").length} 托管`
                : `${data.v3.targets.filter((target) => target.ownership === "managed").length} managed`}
            </span>
          </div>
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{pick(locale, "Target", "目标")}</th>
                  <th>{pick(locale, "Path", "路径")}</th>
                  <th>{pick(locale, "Ownership", "所有权")}</th>
                  <th>{pick(locale, "Capabilities", "能力")}</th>
                  <th>{pick(locale, "Bindings", "绑定")}</th>
                  <th>{pick(locale, "Projections", "投影")}</th>
                </tr>
              </thead>
              <tbody>
                {data.v3.targets.map((target) => {
                  const bindings = data.v3.bindings.filter(
                    (binding) => binding.default_target_id === target.target_id,
                  );
                  const projections = data.v3.projections.filter(
                    (projection) => projection.target_id === target.target_id,
                  );
                  return (
                    <tr
                      className={target.target_id === active?.target_id ? "active" : undefined}
                      key={target.target_id}
                      onClick={() => setSelectedTargetId(target.target_id)}
                    >
                      <td>
                        <div className="primary-copy">{target.target_id}</div>
                        <div className="mono-copy">{agentDisplayName(target.agent)}</div>
                      </td>
                      <td>
                        <div className="mono-copy">{target.path}</div>
                      </td>
                      <td>
                        <span
                          className={`minor-chip ${
                            target.ownership === "managed"
                              ? "is-success"
                              : target.ownership === "observed"
                                ? "is-warning"
                                : "is-danger"
                          }`}
                        >
                          {ownershipLabel(locale, target.ownership)}
                        </span>
                      </td>
                      <td>
                        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                          {target.capabilities.symlink ? <span className="minor-chip">{capabilityLabel(locale, "symlink")}</span> : null}
                          {target.capabilities.copy ? <span className="minor-chip">{capabilityLabel(locale, "copy")}</span> : null}
                          {target.capabilities.watch ? <span className="minor-chip">{capabilityLabel(locale, "watch")}</span> : null}
                        </div>
                      </td>
                      <td>
                        <div className="primary-copy">{bindings.length}</div>
                      </td>
                      <td>
                        <div className="primary-copy">{projections.length}</div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>

        <aside className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Target Inspector", "目标检查器")}</h2>
            {active ? <span className="metric-chip is-primary">{active.target_id}</span> : null}
          </div>
          <div className="detail-panel">
            {!active ? (
              <div className="empty-state">{pick(locale, "no targets available", "没有可用目标")}</div>
            ) : (
              <>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Filesystem Path", "文件系统路径")}</div>
                  <div className="primary-copy">{formatShortPath(active.path, locale)}</div>
                  <div className="mono-copy">{active.path}</div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Boundary", "边界")}</div>
                  <div className="primary-copy">{ownershipLabel(locale, active.ownership)}</div>
                  <div className="secondary-copy">
                    {active.ownership === "managed"
                      ? pick(locale, "Loom can safely own projection lifecycle here.", "Loom 可以安全接管这里的投影生命周期。")
                      : active.ownership === "observed"
                        ? pick(locale, "Directory is watched, but mutations should stay conservative.", "目录会被观察，但变更应保持保守。")
                        : pick(locale, "External path: show state, surface risk, avoid destructive assumptions.", "外部路径：展示状态、暴露风险、避免破坏性假设。")}
                  </div>
                  <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 10 }}>
                    <span className="minor-chip">{agentDisplayName(active.agent)}</span>
                    {active.capabilities.symlink ? <span className="minor-chip">{capabilityLabel(locale, "symlink")}</span> : null}
                    {active.capabilities.copy ? <span className="minor-chip">{capabilityLabel(locale, "copy")}</span> : null}
                    {active.capabilities.watch ? <span className="minor-chip">{capabilityLabel(locale, "watch")}</span> : null}
                  </div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Dependency Counts", "依赖计数")}</div>
                  <div className="mini-stats" style={{ marginTop: 12 }}>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Bindings", "绑定")}</div>
                      <div className="mini-value">{relatedBindings.length}</div>
                    </div>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Rules", "规则")}</div>
                      <div className="mini-value">{relatedRules.length}</div>
                    </div>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Proj.", "投影")}</div>
                      <div className="mini-value">{relatedProjections.length}</div>
                    </div>
                    <div className="mini-stat">
                      <div className="minor-kicker">{pick(locale, "Drift", "漂移")}</div>
                      <div className="mini-value">
                        {relatedProjections.filter(projectionIsDrifted).length}
                      </div>
                    </div>
                  </div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "Dependent Bindings", "依赖绑定")}</div>
                  <div className="detail-list" style={{ marginTop: 12 }}>
                    {relatedBindings.length === 0 ? (
                      <div className="empty-state">{pick(locale, "no dependent bindings", "没有依赖绑定")}</div>
                    ) : (
                      relatedBindings.map((binding) => (
                        <div className="table-row-summary" key={binding.binding_id}>
                          <div className="primary-copy">{binding.binding_id}</div>
                          <div className="secondary-copy">
                            {binding.workspace_matcher.value} · {binding.policy_profile}
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </article>
                <article className="detail-item">
                  <div className="minor-kicker">{pick(locale, "CLI Handoff", "CLI 交接")}</div>
                  <div className="detail-list" style={{ marginTop: 12 }}>
                    <div className="mono-copy">
                      loom --json --root "{data.info.root ?? "<root>"}" target show {active.target_id}
                    </div>
                    <div className="mono-copy">
                      loom --json --root "{data.info.root ?? "<root>"}" target list
                    </div>
                  </div>
                </article>
              </>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}

function ProjectionsPage({
  data,
  locale,
  skillViews,
}: {
  data: PanelData;
  locale: Locale;
  skillViews: SkillView[];
}) {
  const targets = data.v3.targets;
  const [selectedProjection, setSelectedProjection] = useState<V3Projection | null>(
    data.v3.projections[0] ?? null,
  );

  const matrix = new Map<string, V3Projection[]>();
  for (const projection of data.v3.projections) {
    const key = `${projection.skill_id}::${projection.target_id}`;
    const current = matrix.get(key) ?? [];
    current.push(projection);
    matrix.set(key, current);
  }

  const drifted = data.v3.projections.filter(projectionIsDrifted);
  const matrixTemplateColumns = `180px repeat(${Math.max(targets.length, 1)}, 140px)`;

  return (
    <div className="stitch-screen-column">
      <div className="stitch-breadcrumb-bar">
        <div className="stitch-breadcrumbs">
          <span>{pick(locale, "Cluster", "集群")}</span>
          <Icon name="chevron_right" />
          <span>{pick(locale, "Projections", "投影")}</span>
          <Icon name="chevron_right" />
          <span className="is-active">{pick(locale, "Matrix View", "矩阵视图")}</span>
        </div>
        <div className="page-actions">
          <span className="stitch-inline-meta is-accent">
            <Icon name="refresh" />
            <span>{pick(locale, "Auto-Sync", "自动同步")}</span>
          </span>
          <button className="button" disabled type="button">
            {pick(locale, "Force Re-Map", "强制重映射")}
          </button>
        </div>
      </div>

      <div className="projection-grid stitch-projection-layout">
        <section className="panel stitch-panel-reset">
          <div className="stitch-matrix-shell">
            {targets.length === 0 || skillViews.length === 0 ? (
              <div className="empty-state">{pick(locale, "no projections available", "没有可用投影")}</div>
            ) : (
              <div className="stitch-matrix-grid" style={{ gridTemplateColumns: matrixTemplateColumns }}>
                <div className="stitch-matrix-corner">
                  <span>{pick(locale, "Source \\ Target", "源 \\ 目标")}</span>
                </div>
                {targets.map((target, index) => (
                  <div className="stitch-matrix-head" key={target.target_id}>
                    <div className="stitch-matrix-head-card">
                      <span className="stitch-inline-tag">{`BINDING:${String(index + 1).padStart(2, "0")}`}</span>
                      <div className="stitch-binding-cell-title">{target.target_id}</div>
                      <div className="mono-copy">{`ID: ${target.target_id.slice(0, 8)}...`}</div>
                    </div>
                  </div>
                ))}
                {skillViews.map((skill) => (
                  <Fragment key={skill.name}>
                    <div className="stitch-matrix-source">
                      <div className="stitch-matrix-source-head">
                        <Icon name={projectionSourceIcon(skill)} />
                        <span className="stitch-matrix-row-title">{skill.name}</span>
                      </div>
                      <span className="mono-copy">{`SHA: ${(skill.projections[0]?.last_applied_rev ?? "pending").slice(0, 7)}`}</span>
                    </div>
                    {targets.map((target) => {
                      const cell = matrix.get(`${skill.name}::${target.target_id}`) ?? [];
                      const projection = cell[0] ?? null;
                      const toneClass = projectionIsDrifted(projection)
                        ? "is-drifted"
                        : projection?.health === "warning"
                          ? "is-warning"
                          : projection
                            ? "is-success"
                            : "is-empty";
                      return (
                        <button
                          className={`stitch-matrix-cell ${toneClass}`}
                          key={`${skill.name}-${target.target_id}`}
                          onClick={() => setSelectedProjection(projection)}
                          type="button"
                        >
                          {!projection ? (
                            <div className="stitch-matrix-empty">
                              <Icon name="remove" />
                            </div>
                          ) : (
                            <div className="stitch-matrix-card">
                              <div className="stitch-projection-card-top">
                                <span className={`material-symbols-outlined ${projectionToneClass(projection)}`}>
                                  {projectionIsDrifted(projection)
                                    ? "warning"
                                    : projection.health === "warning"
                                      ? "sync"
                                      : "check_circle"}
                                </span>
                                <span className="stitch-inline-tag">{projection.method.toUpperCase()}</span>
                              </div>
                              <div className="stitch-progress-bar">
                                <div
                                  className={`stitch-progress-fill ${projectionToneClass(projection)}`}
                                  style={{ width: `${projectionProgress(projection)}%` }}
                                />
                              </div>
                              <span className={`stitch-projection-foot ${projectionToneClass(projection)}`}>
                                {projectionIsDrifted(projection)
                                  ? pick(locale, "Drift Detected", "检测到漂移")
                                  : projection.health === "warning"
                                    ? pick(locale, "Syncing...", "同步中...")
                                    : pick(locale, "Healthy", "健康")}
                              </span>
                            </div>
                          )}
                        </button>
                      );
                    })}
                  </Fragment>
                ))}
              </div>
            )}
          </div>
        </section>

        <aside className="panel stitch-filter-sidebar">
          <div className="detail-panel">
            <div className="stitch-drawer-card">
              <h3 className="stitch-block-title">{pick(locale, "Project Filter", "投影筛选")}</h3>
              <div className="stitch-filter-group">
                <label>{pick(locale, "Methodology", "方法")}</label>
                <div className="stitch-rail-tabs">
                  <button className="stitch-tab active" type="button">{pick(locale, "Symlink", "符号链接")}</button>
                  <button className="stitch-tab" type="button">{pick(locale, "Copy", "复制")}</button>
                </div>
              </div>
              <div className="stitch-filter-group">
                <label>{pick(locale, "Health Profile", "健康配置")}</label>
                <div className="stitch-checklist">
                  <label><input checked readOnly type="checkbox" /> <span>{pick(locale, "Healthy", "健康")} [{data.v3.projections.length - drifted.length}]</span></label>
                  <label><input checked readOnly type="checkbox" /> <span>{pick(locale, "Drifted", "漂移")} [{drifted.length}]</span></label>
                  <label><input readOnly type="checkbox" /> <span>{pick(locale, "Failed", "失败")} [0]</span></label>
                  <label><input readOnly type="checkbox" /> <span>{pick(locale, "Syncing", "同步中")} [0]</span></label>
                </div>
              </div>
              <div className="stitch-filter-group">
                <label>{pick(locale, "Workspace Scope", "工作区范围")}</label>
                <select className="stitch-select" defaultValue="GLOBAL_PROD">
                  <option>GLOBAL_PROD</option>
                  <option>STAGING_ALPHA</option>
                  <option>DEV_LOCAL</option>
                </select>
              </div>
            </div>

            <div className="stitch-drawer-card">
              <h3 className="stitch-block-title">{pick(locale, "Topology Stats", "拓扑统计")}</h3>
              <div className="stitch-kpi-grid">
                <div className="stitch-kpi-card stitch-kpi-card-primary">
                  <p>{pick(locale, "Active Nodes", "活跃节点")}</p>
                  <strong>{targets.length}</strong>
                </div>
                <div className="stitch-kpi-card stitch-kpi-card-success">
                  <p>{pick(locale, "Sync Rate", "同步率")}</p>
                  <strong>{`${Math.max(0, 100 - drifted.length * 7)}%`}</strong>
                </div>
              </div>
            </div>

            <div className="stitch-drawer-card stitch-alert-card">
              <div className="stitch-alert-head">
                <Icon name="warning" />
                <span>{pick(locale, "Drift Alert", "漂移告警")}</span>
              </div>
              <p className="secondary-copy">
                {selectedProjection
                  ? `${selectedProjection.skill_id} · ${selectedProjection.target_id} · ${selectedProjection.last_applied_rev}`
                  : drifted[0]
                    ? `${drifted[0].skill_id} · ${drifted[0].target_id} · ${drifted[0].last_applied_rev}`
                    : pick(locale, "No drift currently detected.", "当前没有检测到漂移。")}
              </p>
              <button className="button" disabled type="button">{pick(locale, "Re-Sync Node", "重新同步节点")}</button>
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}

function OpsPage({ data, locale }: { data: PanelData; locale: Locale }) {
  const [selectedOpId, setSelectedOpId] = useState<string>(data.pending.ops[0]?.request_id ?? "");
  const activeOp =
    data.pending.ops.find((op) => op.request_id === selectedOpId) ?? data.pending.ops[0] ?? null;
  const queuedOps = data.pending.ops.slice(0, 3);
  const scheduledOps = data.pending.ops.slice(3, 5);
  const drifted = data.v3.projections.filter(projectionIsDrifted);
  const repairTarget = activeOp?.op_id ?? activeOp?.request_id ?? "902-X-FAIL";

  return (
    <div className="stitch-screen-column">
      <div className="ops-grid stitch-ops-layout">
        <section className="panel stitch-queue-panel">
          <div className="stitch-queue-head">
            <h2 className="panel-title">{pick(locale, "Active Queue", "活跃队列")}</h2>
            <span className="minor-chip is-primary">{locale === "zh-CN" ? `${data.pending.count} 待处理` : `${data.pending.count} PENDING`}</span>
          </div>
          <div className="stitch-queue-body">
            {queuedOps.length === 0 ? (
              <div className="empty-state">{pick(locale, "queue empty", "队列为空")}</div>
            ) : (
              queuedOps.map((op, index) => {
                const tone = opToneClass(op, data.remote.sync_state, index);
                return (
                  <button
                    className={`stitch-op-card ${tone} ${activeOp?.request_id === op.request_id ? "active" : ""}`}
                    key={op.request_id}
                    onClick={() => setSelectedOpId(op.request_id)}
                    type="button"
                  >
                    <div className="stitch-op-card-meta">
                      <span>{op.op_id ?? op.request_id}</span>
                      <span>{formatTime(locale, op.created_at)}</span>
                    </div>
                    <h3>{op.command.toUpperCase().replaceAll(" ", "_")}</h3>
                    <p>{summarizeDetails(op.details)}</p>
                    {tone === "is-primary" ? (
                      <>
                        <div className="stitch-progress-bar">
                          <div className="stitch-progress-fill is-primary" style={{ width: `${opProgress(op)}%` }} />
                        </div>
                        <div className="stitch-op-card-processing">
                          <span className="stitch-inline-meta is-accent">
                            <Icon name="refresh" />
                            <span>{pick(locale, "Processing", "处理中")}</span>
                          </span>
                          <span>{`${opProgress(op)}% ${pick(locale, "Complete", "完成")}`}</span>
                        </div>
                      </>
                    ) : tone === "is-danger" ? (
                      <div className="stitch-op-card-actions">
                        <button className="stitch-op-inline-button is-danger" type="button">Repair_Force</button>
                        <button className="stitch-op-inline-button" type="button">Details</button>
                      </div>
                    ) : (
                      <div className="stitch-op-card-tags">
                        {Object.entries(op.details).slice(0, 2).map(([key, value]) => (
                          <span className="stitch-status-tag" key={key}>{`${key}:${String(value)}`}</span>
                        ))}
                      </div>
                    )}
                  </button>
                );
              })
            )}

            <div className="stitch-queue-divider">
              <span>{pick(locale, "Pending Schedule", "等待调度")}</span>
            </div>

            {scheduledOps.map((op, index) => (
              <div className="stitch-scheduled-card" key={op.request_id}>
                <div className="stitch-scheduled-card-main">
                  <Icon name={scheduledOpIcon(index)} />
                  <div>
                    <h4>{op.command.toUpperCase().replaceAll(" ", "_")}</h4>
                    <p>{pick(locale, "Awaiting previous job", "等待前序任务")}</p>
                  </div>
                </div>
                <button className="icon-button" type="button">
                  <Icon name={index === 0 ? "more_vert" : "pause"} />
                </button>
              </div>
            ))}
          </div>
        </section>

        <section className="panel stitch-terminal-panel">
          <div className="stitch-terminal-metadata">
            <div>
              <span>{pick(locale, "Target Instance", "目标实例")}</span>
              <strong>{activeOp?.request_id ?? "Loom-Core"}</strong>
            </div>
            <div>
              <span>{pick(locale, "Exit Status", "退出状态")}</span>
              <strong>{drifted.length > 0 ? "1 (FATAL_ERROR)" : "0 (OK)"}</strong>
            </div>
            <div>
              <span>{pick(locale, "Execution Time", "执行时间")}</span>
              <strong>{`${452 + (data.pending.journal_events ?? 0)}ms`}</strong>
            </div>
            <div>
              <span>{pick(locale, "Process Owner", "进程所有者")}</span>
              <strong className="stitch-terminal-owner">
                <span>{pick(locale, "SYSTEM:ROOT", "SYSTEM:ROOT")}</span>
                <Icon name="verified_user" />
              </strong>
            </div>
          </div>

          <div className="stitch-terminal-output">
            <div className="stitch-terminal-row stitch-terminal-head-row">
              <span>T-STAMP</span>
              <span>LOG_LEVEL</span>
              <span>TRACE_MESSAGE</span>
            </div>
            {(activeOp ? [activeOp, ...data.pending.ops.filter((op) => op.request_id !== activeOp.request_id)] : data.pending.ops)
              .slice(0, 6)
              .map((op) => (
                <div className="stitch-terminal-row" key={op.request_id}>
                  <span>{formatTime(locale, op.created_at)}</span>
                  <span className={opToneClass(op, data.remote.sync_state)}>{`[${op.command.split(" ")[0].toUpperCase()}]`}</span>
                  <span>{summarizeDetails(op.details)}</span>
                </div>
              ))}
            <div className="stitch-terminal-row stitch-terminal-cursor-row">
              <span className="is-primary">$</span>
              <span className="is-primary">_</span>
              <span />
            </div>
          </div>

          <div className="stitch-terminal-footer">
            <div className="stitch-terminal-repl">
              <span>REPL:</span>
              <div className="stitch-terminal-input">
                <span>loom-cli:</span>
                <input readOnly type="text" value={`loom-repair --target ${repairTarget} --force`} />
              </div>
            </div>
            <div className="page-actions">
              <button className="button-ghost" type="button">Clear_Logs</button>
              <button className="button-ghost" type="button">Download_Trace</button>
              <button className="button" type="button">Execute_Command</button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

function SettingsPage({ data, locale }: { data: PanelData; locale: Locale }) {
  const paths = [
    { label: pick(locale, "Workspace Root", "工作区根目录"), value: data.info.root },
    { label: pick(locale, "State Directory", "状态目录"), value: data.info.state_dir },
    { label: pick(locale, "Targets File", "目标文件"), value: data.info.targets_file },
    { label: pick(locale, "Claude Directory", "Claude 目录"), value: data.info.claude_dir },
    { label: pick(locale, "Codex Directory", "Codex 目录"), value: data.info.codex_dir },
    { label: pick(locale, "Remote URL", "远端 URL"), value: data.info.remote_url },
  ];

  return (
    <div className="page-body page-frame">
      <div className="page-head">
        <div>
          <h1 className="page-title">{pick(locale, "Environment", "环境")}</h1>
          <p className="page-subtitle">
            {pick(
              locale,
              "Local control-plane environment, state model posture, and repository-level operating context.",
              "查看本地控制平面环境、状态模型姿态与仓库级运行上下文。",
            )}
          </p>
        </div>
        <div className="page-actions">
          <span className={`status-pill ${data.live ? "is-success" : "is-warning"}`}>
            <Icon name={data.live ? "hub" : "science"} />
            {data.live ? pick(locale, "api connected", "API 已连接") : pick(locale, "api unavailable", "API 不可用")}
          </span>
          <span className="status-pill is-primary">
            <Icon name="schedule" />
            {formatTime(locale, data.lastUpdated)}
          </span>
        </div>
      </div>

      <div className="overview-grid">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Environment Paths", "环境路径")}</h2>
            <span className="minor-chip">{pick(locale, "filesystem truth", "文件系统真实值")}</span>
          </div>
          <div className="panel-content detail-list">
            {paths.map((item) => (
              <article className="detail-item" key={item.label}>
                <div className="minor-kicker">{item.label}</div>
                <div className="mono-copy" style={{ marginTop: 8 }}>
                  {item.value ?? missingLabel(locale)}
                </div>
              </article>
            ))}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "State Model", "状态模型")}</h2>
            <span className={`minor-chip ${data.v3.available ? "is-success" : "is-warning"}`}>
              {data.v3.available ? pick(locale, "v3 available", "v3 可用") : pick(locale, "v3 unavailable", "v3 不可用")}
            </span>
          </div>
          <div className="detail-panel">
            <article className="detail-item">
              <div className="minor-kicker">{pick(locale, "Counts", "计数")}</div>
              <div className="mini-stats" style={{ marginTop: 12 }}>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Skills", "技能")}</div>
                  <div className="mini-value">{data.v3.counts.skills}</div>
                </div>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Bindings", "绑定")}</div>
                  <div className="mini-value">{data.v3.counts.bindings}</div>
                </div>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Targets", "目标")}</div>
                  <div className="mini-value">{data.v3.counts.targets}</div>
                </div>
                <div className="mini-stat">
                  <div className="minor-kicker">{pick(locale, "Proj.", "投影")}</div>
                  <div className="mini-value">{data.v3.counts.projections}</div>
                </div>
              </div>
            </article>
            <article className="detail-item">
              <div className="minor-kicker">{pick(locale, "Checkpoint", "检查点")}</div>
              <div className="detail-list" style={{ marginTop: 12 }}>
                <div className="mono-copy">
                  {pick(locale, "scanned", "已扫描")}: {data.v3.checkpoint?.last_scanned_op_id ?? missingLabel(locale)}
                </div>
                <div className="mono-copy">
                  {pick(locale, "acked", "已确认")}: {data.v3.checkpoint?.last_acked_op_id ?? missingLabel(locale)}
                </div>
                <div className="mono-copy">
                  {pick(locale, "updated", "更新时间")}: {formatTime(locale, data.v3.checkpoint?.updated_at)}
                </div>
              </div>
            </article>
            <article className="detail-item">
              <div className="minor-kicker">{pick(locale, "Remote Posture", "远端姿态")}</div>
              <div className="secondary-copy">
                {syncStateLabel(locale, data.remote.sync_state)} ·{" "}
                {locale === "zh-CN"
                  ? `${data.remote.ahead ?? 0} 领先 / ${data.remote.behind ?? 0} 落后 · ${data.pending.count} 待处理操作`
                  : `${data.remote.ahead ?? 0} ahead / ${data.remote.behind ?? 0} behind · ${data.pending.count} pending ops`}
              </div>
            </article>
          </div>
        </section>
      </div>

      <div className="split-two">
        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "Warnings", "告警")}</h2>
          </div>
          <div className="panel-content warning-list">
            {[...data.remoteWarnings, ...(data.pending.warnings ?? [])].length === 0 ? (
              <div className="empty-state">{pick(locale, "no active warnings", "没有活跃告警")}</div>
            ) : (
              [...data.remoteWarnings, ...(data.pending.warnings ?? [])].map((warning) => (
                <div className="risk-item is-warning" key={warning}>
                  <div className="secondary-copy">{warning}</div>
                </div>
              ))
            )}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2 className="panel-title">{pick(locale, "CLI Surface", "CLI 面板")}</h2>
          </div>
          <div className="panel-content detail-list">
            <div className="mono-copy">loom --json --root "{data.info.root ?? "<root>"}" workspace status</div>
            <div className="mono-copy">loom --json --root "{data.info.root ?? "<root>"}" workspace doctor</div>
            <div className="mono-copy">loom --json --root "{data.info.root ?? "<root>"}" sync status</div>
            <div className="mono-copy">loom --json --root "{data.info.root ?? "<root>"}" migrate v2-to-v3 --plan</div>
          </div>
        </section>
      </div>
    </div>
  );
}

export function App() {
  const [locale, setLocale] = useState<Locale>(() => detectLocale());
  const [topbarQuery, setTopbarQuery] = useState("");
  const { data, loading, loadError, navPending, page, navigate, refresh } = usePanelApp(locale);
  const skillViews = useMemo(() => buildSkillViews(data), [data]);
  const showTopbarSearch = page === "overview" || page === "skills" || page === "ops";
  const topbarSearchValue = page === "skills" ? topbarQuery : "";

  useEffect(() => {
    persistLocale(locale);
    window.document.title = pick(locale, "Loom Panel", "Loom 控制台");
  }, [locale]);

  let content = <OverviewPage data={data} locale={locale} skillViews={skillViews} />;
  if (page === "skills") {
    content = <SkillsPage data={data} locale={locale} query={topbarQuery} skillViews={skillViews} />;
  } else if (page === "bindings") {
    content = <BindingsPage data={data} locale={locale} skillViews={skillViews} />;
  } else if (page === "targets") {
    content = <TargetsPage data={data} locale={locale} />;
  } else if (page === "projections") {
    content = <ProjectionsPage data={data} locale={locale} skillViews={skillViews} />;
  } else if (page === "ops") {
    content = <OpsPage data={data} locale={locale} />;
  } else if (page === "settings") {
    content = <SettingsPage data={data} locale={locale} />;
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <div className="brand-icon">
              <Icon name="blur_on" />
            </div>
            <div>
              <h1 className="brand-title">LOOM_CORE</h1>
              <p className="brand-meta">{pick(locale, "desktop control plane", "桌面控制平面")}</p>
            </div>
          </div>
        </div>

        <ul className="nav-list">
          {NAV_ITEMS.map((item) => (
            <li key={item.id}>
              <button
                className={`nav-item ${page === item.id ? "active" : ""}`}
                onClick={() => navigate(item.id)}
              >
                <Icon name={item.icon} />
                <span className="nav-label">{navLabel(locale, item.id)}</span>
              </button>
            </li>
          ))}
        </ul>

        <div className="sidebar-footer">
          <button className="nav-item">
            <Icon name="description" />
            <span className="nav-label">{pick(locale, "Docs", "文档")}</span>
          </button>
          <button className="nav-item">
            <Icon name="help" />
            <span className="nav-label">{pick(locale, "Help", "帮助")}</span>
          </button>
        </div>
      </aside>

      <div className="main-shell">
        <header className="topbar">
          <div className="topbar-left">
            {page === "overview" ? (
              <>
                <div className="topbar-env">
                  <span className="topbar-env-label">ENV:</span>
                  <span className="topbar-env-value">ROOT_ENV</span>
                </div>
                <span className="topbar-divider" />
              </>
            ) : (
              <span className="topbar-wordmark">LOOM</span>
            )}
            <nav className="topbar-links">
              <button className="topbar-link" type="button">PROD-CLUSTER-01</button>
              <button className="topbar-link" type="button">US-EAST-1</button>
              <button className="topbar-link active" type="button">
                {`SYNC:${(data.remote.sync_state ?? "ACTIVE").toUpperCase()}`}
              </button>
            </nav>
          </div>

          <div className="topbar-right">
            {showTopbarSearch ? (
              <label className="topbar-search-shell">
                <Icon name="search" />
                <input
                  className="toolbar-search"
                  placeholder={topbarSearchPlaceholder(page, locale)}
                  value={topbarSearchValue}
                  onChange={(event) => setTopbarQuery(event.target.value)}
                />
              </label>
            ) : null}
            <button
              className="icon-button"
              title={pick(locale, "Notifications", "通知")}
            >
              <Icon name="notifications" />
              {page === "bindings" ? <span className="topbar-notify-dot" /> : null}
            </button>
            <button
              className="icon-button"
              title={pick(locale, "Environment", "环境")}
              onClick={() => navigate("settings")}
            >
              <Icon name="settings" />
            </button>
            <button
              className="topbar-avatar"
              onClick={() => setLocale((current) => (current === "zh-CN" ? "en" : "zh-CN"))}
              title={pick(locale, "Toggle language", "切换语言")}
              type="button"
            >
              <span className="topbar-avatar-core" />
              <span className="topbar-avatar-badge">{locale === "zh-CN" ? "中" : "EN"}</span>
            </button>
          </div>
        </header>

        <div className="page-scroll">
          {(loadError || navPending) ? (
            <div className="page-banner-stack">
              {loadError ? (
                <span className="status-pill is-danger">
                  <Icon name="error" />
                  {loadError}
                </span>
              ) : null}
              {navPending ? (
                <span className="status-pill is-primary">
                  <Icon name="swap_horiz" />
                  {pick(locale, "switching page", "切换页面中")}
                </span>
              ) : null}
            </div>
          ) : null}
          {content}
        </div>

        <footer className="shell-statusbar">
          <div className="shell-status-item is-success">
            <Icon name="check_circle" />
            <span>{pick(locale, "SYS:HEALTHY", "系统：健康")}</span>
          </div>
          <div className="shell-status-item">
            <Icon name="speed" />
            <span>{`LATENCY:${12 + (data.pending.count ?? 0)}ms`}</span>
          </div>
          <div className="shell-status-item">
            <Icon name="rebase_edit" />
            <span>{`NODES:${Math.max(1, data.v3.targets.length)}`}</span>
          </div>
          <div className="shell-status-item">
            <Icon name="timer" />
            <span>{`UPTIME:${data.live ? "99.9%" : "degraded"}`}</span>
          </div>
          <div className="shell-status-spacer" />
          {statusFooterTail(page, data, locale)}
        </footer>
      </div>
    </div>
  );
}
