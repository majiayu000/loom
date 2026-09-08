import { useState } from "react";
import {
  isDesktop,
  native,
  requireSuccess,
  type Envelope,
  type Skill,
  type Version,
} from "./client";
export type Runner = (action: () => Promise<void>) => Promise<void>;

export function Evidence({ value }: { value: Record<string, unknown> }) {
  return (
    <details>
      <summary>查看诊断详情</summary>
      <pre className="operation-result">{JSON.stringify(value, null, 2)}</pre>
    </details>
  );
}

export function Activation({
  skill,
  root,
  run,
}: {
  skill: string;
  root: string;
  run: Runner;
}) {
  const [agent, setAgent] = useState("codex");
  const [workspace, setWorkspace] = useState("");
  const [plan, setPlan] = useState<Record<string, unknown> | null>(null);
  const [installed, setInstalled] = useState(false);
  const [checks, setChecks] = useState<Record<string, unknown> | null>(null);
  const [busy, setBusy] = useState(false);
  const act = (fn: () => Promise<void>) =>
    run(async () => {
      setBusy(true);
      try {
        await fn();
      } finally {
        setBusy(false);
      }
    });
  const args = {
    root: root || null,
    skill,
    agent,
    workspace: workspace || null,
  };
  const reset = () => {
    setPlan(null);
    setInstalled(false);
    setChecks(null);
  };
  return (
    <section className="activation">
      <h3>激活到项目</h3>
      <label>
        目标工具
        <select
          disabled={busy}
          value={agent}
          onChange={(e) => {
            setAgent(e.target.value);
            reset();
          }}
        >
          <option value="codex">Codex</option>
          <option value="claude">Claude Code</option>
          <option value="cursor">Cursor</option>
        </select>
      </label>
      <label>
        项目目录
        <input value={workspace} readOnly placeholder="选择已有项目" />
      </label>
      <button
        type="button"
        disabled={busy}
        onClick={() =>
          void act(async () => {
            const path = await native<string | null>("choose_directory");
            if (path) {
              setWorkspace(path);
              reset();
            }
          })
        }
      >
        选择项目
      </button>
      <button
        type="button"
        disabled={busy || !workspace}
        onClick={() =>
          void act(async () => {
            reset();
            setPlan(
              requireSuccess(await native<Envelope>("preview_activate", args)),
            );
          })
        }
      >
        预览激活
      </button>
      {plan && (
        <div className="team-notice">
          <p>
            工具：{agent} · 项目：{workspace}
          </p>
          <p>
            将写入：
            {String(
              plan.materialized_path ?? plan.target_path ?? "请查看诊断详情",
            )}
          </p>
          <Evidence value={plan} />
          <button
            className="primary"
            type="button"
            disabled={busy || installed}
            onClick={() =>
              void act(async () => {
                requireSuccess(await native<Envelope>("apply_activate", args));
                setInstalled(true);
                setPlan(null);
              })
            }
          >
            确认激活到项目
          </button>
        </div>
      )}
      {installed && (
        <p role="status">文件已激活。请检查依赖和可见性，并新建 Agent 会话。</p>
      )}
      <button
        type="button"
        disabled={busy || !workspace}
        onClick={() =>
          void act(async () => {
            const deps = requireSuccess(
              await native<Envelope>("deps_skill", args),
            );
            const visibility = requireSuccess(
              await native<Envelope>("visibility_skill", args),
            );
            setChecks({ dependencies: deps, visibility });
          })
        }
      >
        检查依赖与可见性
      </button>
      {checks && (
        <div>
          <h4>检查结果</h4>
          <p>
            依赖：
            {(checks.dependencies as Record<string, unknown>).ready === true
              ? "已就绪"
              : String(
                  (checks.dependencies as Record<string, unknown>).status ??
                    "尚未就绪",
                )}
          </p>
          <ul>
            {(
              ((checks.dependencies as Record<string, unknown>).findings ??
                []) as {
                id: string;
                message: string;
                suggested_action: string;
              }[]
            ).map((f) => (
              <li key={f.id}>
                {f.message} {f.suggested_action}
              </li>
            ))}
          </ul>
          <p>
            Agent 可见性：
            {(checks.visibility as Record<string, unknown>).visible === true
              ? "文件配置检查通过，仍需新建会话"
              : "检查未通过"}
          </p>
          <ul>
            {(
              ((checks.visibility as Record<string, unknown>).checks ?? []) as {
                id: string;
                ok: boolean;
                message: string;
              }[]
            )
              .filter((c) => !c.ok)
              .map((c) => (
                <li key={c.id}>{c.message}</li>
              ))}
          </ul>
          <Evidence value={checks} />
        </div>
      )}
    </section>
  );
}

export function InstallSkill({
  team,
  skill,
  versions,
  run,
}: {
  team: string;
  skill: Skill;
  versions: Version[];
  run: Runner;
}) {
  const [root, setRoot] = useState("");
  const [version, setVersion] = useState(
    skill.recommended_version_id ? "recommended" : (versions[0]?.id ?? ""),
  );
  const [plan, setPlan] = useState<Record<string, unknown> | null>(null);
  const [installed, setInstalled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [key, setKey] = useState(() => crypto.randomUUID());
  const fixed =
    version === "recommended" ? skill.recommended_version_id : version;
  const act = (fn: () => Promise<void>) =>
    run(async () => {
      setBusy(true);
      try {
        await fn();
      } finally {
        setBusy(false);
      }
    });
  const reset = () => {
    setPlan(null);
    setInstalled(false);
    setKey(crypto.randomUUID());
  };
  return (
    <div className="team-card install-card">
      <h2>安装到我的工具</h2>
      <label>
        版本
        <select
          disabled={busy}
          value={version}
          onChange={(e) => {
            setVersion(e.target.value);
            reset();
          }}
        >
          {skill.recommended_version_id && (
            <option value="recommended">跟随推荐版本（手动更新）</option>
          )}
          {versions.map((v) => (
            <option key={v.id} value={v.id}>
              固定 v{v.version}
            </option>
          ))}
        </select>
      </label>
      {isDesktop() ? (
        <>
          <label>
            Registry 目录（留空使用默认）
            <input
              value={root}
              disabled={busy}
              onChange={(e) => {
                setRoot(e.target.value);
                reset();
              }}
            />
          </label>
          <p className="subtle">
            先导入本机仓库，再选择项目激活。更新会检查现有安装与本地修改。
          </p>
          <button
            type="button"
            className="primary"
            disabled={busy || !fixed || !!skill.archived_at}
            onClick={() =>
              void act(async () => {
                reset();
                setPlan(
                  requireSuccess(
                    await native<Envelope>("preview_team_install", {
                      team,
                      skill: skill.id,
                      version: fixed,
                      name: skill.slug,
                      root: root || null,
                      requestedRef: version,
                    }),
                  ),
                );
              })
            }
          >
            预览安装
          </button>
          {plan && (
            <div className="team-notice">
              <h3>安装预览</h3>
              <p>
                {skill.title || skill.slug} ·{" "}
                {versions.find((v) => v.id === fixed)?.version ?? fixed}
              </p>
              <p>目标仓库：{root || "Loom 默认 registry"}</p>
              <p>
                来源已按云端 SHA-256
                校验。确认后将执行此计划，状态变化时引擎会拒绝写入。
              </p>
              <ul>
                {((plan.conflicts ?? []) as { message: string }[]).map(
                  (item) => (
                    <li key={item.message}>{item.message}</li>
                  ),
                )}
              </ul>
              <p>
                {plan.safe_to_apply === true && plan.execution_enabled === true
                  ? "引擎预检允许执行。"
                  : "此计划被策略或冲突阻止，请查看诊断详情和引擎要求，处理后重新预览。"}
              </p>
              <ul>
                {(
                  (plan.effects ?? plan.projections ?? []) as {
                    materialized_path: string;
                    effect: string;
                  }[]
                ).map((item) => (
                  <li key={item.materialized_path}>
                    {item.effect} · {item.materialized_path}
                  </li>
                ))}
              </ul>
              <p>
                所需审批：
                {((plan.required_approvals ?? []) as string[]).join("、") ||
                  "无"}
              </p>
              <Evidence value={plan} />
              <button
                type="button"
                className="primary"
                disabled={
                  busy ||
                  !plan.plan_id ||
                  !plan.plan_digest ||
                  plan.safe_to_apply !== true ||
                  plan.execution_enabled !== true
                }
                onClick={() =>
                  void act(async () => {
                    requireSuccess(
                      await native<Envelope>("apply_plan", {
                        root: root || null,
                        planId: plan.plan_id,
                        planDigest: plan.plan_digest,
                        idempotencyKey: key,
                      }),
                    );
                    setInstalled(true);
                    setPlan(null);
                  })
                }
              >
                确认导入本机仓库
              </button>
            </div>
          )}
          {installed && (
            <p role="status">已导入本机仓库。现在可以激活到项目。</p>
          )}
          {installed && (
            <Activation
              key={`${skill.id}:${root}`}
              skill={skill.slug}
              root={root}
              run={run}
            />
          )}
        </>
      ) : (
        <p className="subtle">
          本机安装由桌面 App 完成。打开 Loom 后，在同一团队选择此技能和版本。
        </p>
      )}
    </div>
  );
}

interface LocalSkill {
  skill_id: string;
  description?: string;
  source_path?: string;
  source_status?: string;
  trust?: string;
  warnings?: string[];
}
export function LocalSkills({ run }: { run: Runner }) {
  const [root, setRoot] = useState("");
  const [result, setResult] = useState<Record<string, unknown> | null>(null);
  const [selected, setSelected] = useState<LocalSkill | null>(null);
  const [inspection, setInspection] = useState<Record<string, unknown> | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const act = (fn: () => Promise<void>) =>
    run(async () => {
      setBusy(true);
      try {
        await fn();
      } finally {
        setBusy(false);
      }
    });
  const read = async () => {
    setSelected(null);
    setInspection(null);
    setResult(
      requireSuccess(
        await native<Envelope>("local_skills", { root: root || null }),
      ),
    );
  };
  return (
    <section className="team-content">
      <span className="eyebrow">ON YOUR MACHINE</span>
      <h1>
        你的技能，
        <br />
        留在你的电脑上。
      </h1>
      <p className="lead">检查本机技能，然后决定哪些值得分享。</p>
      {isDesktop() ? (
        <div className="team-card">
          <label>
            Registry 目录（留空使用 Loom 默认目录）
            <input
              disabled={busy}
              value={root}
              onChange={(e) => {
                setRoot(e.target.value);
                setResult(null);
                setSelected(null);
                setInspection(null);
              }}
              placeholder="使用默认 registry"
            />
          </label>
          <button
            type="button"
            className="primary"
            disabled={busy}
            onClick={() => void act(read)}
          >
            读取本机技能
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void act(async () => {
                requireSuccess(
                  await native<Envelope>("initialize_registry", {
                    root: root || null,
                  }),
                );
                await read();
              })
            }
          >
            初始化 Registry
          </button>
          {result && (
            <>
              <p>
                {result.registry_available === false
                  ? "此目录尚未初始化 registry。"
                  : `共 ${(result.skills as LocalSkill[]).length} 个技能`}
              </p>
              {(result.skills as LocalSkill[]).map((s) => (
                <button
                  type="button"
                  className="skill-card"
                  key={s.skill_id}
                  onClick={() =>
                    void act(async () => {
                      setSelected(s);
                      setInspection(null);
                      setInspection(
                        requireSuccess(
                          await native<Envelope>("inspect_skill", {
                            root: root || null,
                            skill: s.skill_id,
                          }),
                        ),
                      );
                    })
                  }
                >
                  <h3>{s.skill_id}</h3>
                  <p>{s.description}</p>
                  <small>
                    {s.source_status} · {s.source_path}
                  </small>
                </button>
              ))}
            </>
          )}
          {selected && (
            <section>
              <h2>{selected.skill_id}</h2>
              <p>{selected.description}</p>
              {inspection && <Evidence value={inspection} />}
              <Activation
                key={`${root}:${selected.skill_id}`}
                root={root}
                skill={selected.skill_id}
                run={run}
              />
            </section>
          )}
        </div>
      ) : (
        <div className="empty-state">
          <span className="empty-symbol">⌘</span>
          <h2>在桌面 App 中管理本机技能</h2>
          <p>网页可以浏览团队内容，本机文件由桌面 App 读取和管理。</p>
        </div>
      )}
    </section>
  );
}
