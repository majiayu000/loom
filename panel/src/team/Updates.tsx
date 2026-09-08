import { useEffect, useRef, useState } from "react";
import {
  isDesktop,
  native,
  requireSuccess,
  type Envelope,
  type Skill,
} from "./client";
import type { Runner } from "./operations";

interface Installed {
  name: string;
  team: {
    service_origin: string;
    team_id: string;
    skill_id: string;
    version_id: string;
    requested_ref: string;
  } | null;
}

export function Updates({
  team,
  origin,
  skills,
  run,
  open,
}: {
  team: string;
  origin: string;
  skills: Skill[];
  run: Runner;
  open: (skill: Skill, root: string, localName?: string) => void;
}) {
  const [root, setRoot] = useState("");
  const [installed, setInstalled] = useState<Installed[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const [error, setError] = useState("");
  const generation = useRef(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: A changed team, page, origin, or registry invalidates the pending comparison.
  useEffect(() => {
    generation.current++;
    setInstalled(null);
    setProgress("");
    setError("");
    setBusy(false);
    return () => {
      generation.current++;
    };
  }, [team, origin, skills, root]);

  const compare = () =>
    run(async () => {
      const current = generation.current;
      setBusy(true);
      setInstalled(null);
      setError("");
      try {
        if (!origin) throw new Error("请先配置团队服务地址，再对比安装来源。");
        const inventory = requireSuccess(
          await native<Envelope>("local_skills", { root: root || null }),
        );
        if (current !== generation.current) return;
        const rows = inventory.skills as { skill_id: string }[];
        const found: Installed[] = [];
        for (const [index, row] of rows.entries()) {
          setProgress(`检查本机来源 ${index + 1} / ${rows.length}`);
          const detail = requireSuccess(
            await native<Envelope>("inspect_skill", {
              root: root || null,
              skill: row.skill_id,
            }),
          );
          if (current !== generation.current) return;
          const provenance = detail.provenance as
            | { team?: Installed["team"] }
            | undefined;
          found.push({ name: row.skill_id, team: provenance?.team ?? null });
        }
        setInstalled(found);
        setProgress("");
      } catch (err) {
        if (current === generation.current) {
          setError(err instanceof Error ? err.message : String(err));
          setProgress("");
        }
      } finally {
        if (current === generation.current) setBusy(false);
      }
    });

  return (
    <section className="update-comparison" aria-label="本机版本对比">
      <div className="team-card">
        <h2>当前页版本对比</h2>
        <p className="subtle">
          对比当前页 {skills.length} 个云端技能与所选 Registry
          的安装来源。固定版本不会自动升级；本地修改将在安装预览时检查。
        </p>
        {isDesktop() ? (
          <>
            <label>
              Registry 目录（留空使用 Loom 默认目录）
              <input
                value={root}
                disabled={busy}
                onChange={(e) => setRoot(e.target.value)}
              />
            </label>
            <button
              type="button"
              className="primary"
              disabled={busy || !skills.length}
              onClick={() => void compare()}
            >
              {busy ? "正在对比…" : "检查本机版本"}
            </button>
          </>
        ) : (
          <p>请在桌面 App 中对比本机安装状态。网页仅显示推荐版本。</p>
        )}
        {progress && <p role="status">{progress}</p>}
        {error && <p role="alert">对比未完成：{error}。请处理后重新检查。</p>}
      </div>
      {skills.map((skill) => {
        const matches =
          installed?.filter(
            (row) =>
              row.team?.service_origin.replace(/\/$/, "") ===
                origin.replace(/\/$/, "") &&
              row.team.team_id === team &&
              row.team.skill_id === skill.id,
          ) ?? [];
        const sameName =
          installed?.some((row) => row.name === skill.slug) ?? false;
        return (
          <article className="team-card" key={skill.id}>
            <h3>{skill.title || skill.slug}</h3>
            <p>{skill.description}</p>
            <p>
              推荐版本 ID：
              <span className="checksum">
                {skill.recommended_version_id ?? "暂无推荐版本"}
              </span>
            </p>
            {!installed ? (
              <p>本机状态：尚未检查</p>
            ) : !matches.length ? (
              <p>
                {sameName
                  ? "同名技能来自其他来源，未算作此团队技能的安装。"
                  : "此来源尚未安装。"}
              </p>
            ) : (
              matches.map((row) => (
                <div key={row.name}>
                  <p>本机技能：{row.name}</p>
                  <p>
                    当前版本 ID：
                    <span className="checksum">{row.team?.version_id}</span>
                  </p>
                  <p>
                    {row.team?.requested_ref !== "recommended"
                      ? "固定版本：保留当前选择。"
                      : !skill.recommended_version_id
                        ? "暂无推荐版本，无法比较更新。"
                        : row.team.version_id === skill.recommended_version_id
                          ? "已是推荐版本。"
                          : "可更新：团队推荐版本已变化。"}
                  </p>
                  <button
                    type="button"
                    onClick={() => open(skill, root, row.name)}
                  >
                    更新本机 {row.name}
                  </button>
                </div>
              ))
            )}
            {skill.archived_at && (
              <p>已归档，仅可恢复此设备已安装的同来源技能。</p>
            )}
            {matches.length === 0 && (
              <button type="button" onClick={() => open(skill, root)}>
                查看版本与手动预览
              </button>
            )}
          </article>
        );
      })}
    </section>
  );
}
