import { useEffect, useRef, useState } from "react";
import { request, segment, teamPath, type Skill, type Version } from "./client";
import { InstallSkill, type Runner } from "./operations";
import { PublishForm } from "./PublishForm";

function VersionFiles({ path, run }: { path: string; run: Runner }) {
  const [files, setFiles] = useState<{ path: string; size: number }[] | null>(
    null,
  );
  const [text, setText] = useState<{
    path: string;
    text: string | null;
  } | null>(null);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current++;
    },
    [],
  );
  return (
    <section>
      <button
        type="button"
        onClick={() =>
          void run(async () => {
            const current = ++generation.current;
            const data = await request<{
              files: { path: string; size: number }[];
            }>(`${path}/files`);
            if (current === generation.current) setFiles(data.files);
          })
        }
      >
        查看版本文件
      </button>
      {files && (
        <ul>
          {files.map((file) => (
            <li key={file.path}>
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    const current = ++generation.current;
                    const data = await request<{ text: string | null }>(
                      `${path}/file?${new URLSearchParams({ path: file.path })}`,
                    );
                    if (current === generation.current)
                      setText({ path: file.path, text: data.text });
                  })
                }
              >
                {file.path} · {file.size} 字节
              </button>
            </li>
          ))}
        </ul>
      )}
      {text && (
        <div>
          <h4>{text.path}</h4>
          {text.text === null ? (
            <p>此文件为二进制或超过文本预览上限。</p>
          ) : (
            <pre className="example">{text.text}</pre>
          )}
        </div>
      )}
    </section>
  );
}
export function SkillDetail({
  team,
  initial,
  initialRoot = "",
  owner,
  userId,
  run,
  back,
  notice,
}: {
  team: string;
  initial: Skill;
  initialRoot?: string;
  owner: boolean;
  userId: string;
  run: Runner;
  back: () => void;
  notice: (value: string) => void;
}) {
  const [skill, setSkill] = useState(initial);
  const [versions, setVersions] = useState<Version[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [publishing, setPublishing] = useState(false);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  const path = `${teamPath(team)}/skills/${segment(initial.id)}`;
  const allowed = owner || skill.maintainer_id === userId;
  const act = (fn: () => Promise<void>) =>
    run(async () => {
      setBusy(true);
      try {
        await fn();
      } finally {
        setBusy(false);
      }
    });
  const refresh = async (cursor?: string) => {
    const current = generation.current;
    const [detail, data] = await Promise.all([
      request<{ skill: Skill }>(path),
      request<{ versions: Version[]; next_cursor?: string | null }>(
        `${path}/versions?${new URLSearchParams(cursor ? { cursor, limit: "50" } : { limit: "50" })}`,
      ),
    ]);
    if (current !== generation.current) return;
    setSkill(detail.skill);
    setVersions((old) => (cursor ? [...old, ...data.versions] : data.versions));
    setNext(data.next_cursor ?? null);
  };
  useEffect(() => {
    void run(async () => {
      const current = generation.current;
      const [detail, data] = await Promise.all([
        request<{ skill: Skill }>(path),
        request<{ versions: Version[]; next_cursor?: string | null }>(
          `${path}/versions?limit=50`,
        ),
      ]);
      if (current !== generation.current) return;
      setSkill(detail.skill);
      setVersions(data.versions);
      setNext(data.next_cursor ?? null);
    });
    return () => {
      generation.current++;
    };
  }, [path, run]);
  const patch = async (body: unknown) => {
    const data = await request<{ skill: Skill }>(path, {
      method: "PATCH",
      body,
      ifMatch: skill.revision,
    });
    setSkill(data.skill);
  };
  return (
    <section className="team-content">
      <button type="button" className="text-button" onClick={back}>
        ← 返回技能库
      </button>
      <div className="page-heading">
        <div>
          <span className="eyebrow">TEAM SKILL</span>
          <h1>{skill.title || skill.slug}</h1>
          <p className="lead">{skill.description}</p>
          {skill.archived_at && <span className="badge">已归档</span>}
        </div>
        <span className="skill-glyph">↗</span>
      </div>
      <div className="detail-grid">
        <div>
          <div className="team-card">
            <span className="eyebrow">TRY THIS</span>
            <h2>试着这样使用</h2>
            <pre className="example">
              {skill.example || "作者尚未填写示例。"}
            </pre>
            <button
              type="button"
              disabled={!skill.example}
              onClick={() =>
                void act(async () => {
                  await navigator.clipboard.writeText(skill.example);
                  notice(
                    "使用示例已复制。请在安装后新建 Agent 会话，再粘贴使用。",
                  );
                })
              }
            >
              复制使用示例
            </button>
          </div>
          <h2 className="section-title">
            版本历史 <span>{versions.length}</span>
          </h2>
          {versions.map((v) => (
            <div className="team-card" key={v.id}>
              <div className="version-row">
                <div>
                  <strong>v{v.version}</strong>
                  {v.id === skill.recommended_version_id && (
                    <span className="badge">推荐版本</span>
                  )}
                  <p>{v.release_notes || "暂无变更说明"}</p>
                  <small>
                    {new Date(v.created_at).toLocaleDateString("zh-CN")}
                  </small>
                </div>
                <span className="checksum" title={v.sha256}>
                  {v.sha256.slice(0, 10)}
                </span>
              </div>
              <VersionFiles
                path={`${path}/versions/${segment(v.id)}`}
                run={run}
              />
              {allowed && v.id !== skill.recommended_version_id && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      const data = await request<{ skill: Skill }>(
                        `${path}/recommendation`,
                        {
                          method: "PUT",
                          body: {
                            version_id: v.id,
                            expected_version_id: skill.recommended_version_id,
                          },
                        },
                      );
                      setSkill(data.skill);
                    })
                  }
                >
                  设为推荐版本
                </button>
              )}
            </div>
          ))}
          {next && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void act(() => refresh(next))}
            >
              加载更多版本
            </button>
          )}
        </div>
        <div>
          {versions.length > 0 && (
            <InstallSkill
              key={`${skill.id}:${skill.recommended_version_id}`}
              team={team}
              skill={skill}
              initialRoot={initialRoot}
              versions={versions}
              run={run}
            />
          )}
          <div className="detail-meta">
            <span>由团队维护</span>
            <strong>{skill.slug}</strong>
            <p>安装成功后，仍需检查依赖和 Agent 可见性。</p>
          </div>
          {allowed && (
            <>
              <button
                type="button"
                disabled={busy || !!skill.archived_at}
                onClick={() => setPublishing(true)}
              >
                发布新版本
              </button>
              <button type="button" onClick={() => setEditing(!editing)}>
                编辑介绍
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void act(() => patch({ archived: !skill.archived_at }))
                }
              >
                {skill.archived_at ? "取消归档" : "归档技能"}
              </button>
            </>
          )}
        </div>
      </div>
      {editing && (
        <form
          className="team-card"
          key={skill.revision}
          onSubmit={(e) => {
            e.preventDefault();
            const fields = new FormData(e.currentTarget);
            void act(async () => {
              await patch({
                title: fields.get("title"),
                description: fields.get("description"),
                example: fields.get("example"),
                ...(owner && fields.get("maintainer_id")
                  ? { maintainer_id: fields.get("maintainer_id") }
                  : {}),
              });
              setEditing(false);
            });
          }}
        >
          <h2>编辑技能介绍</h2>
          <label>
            显示名称
            <input name="title" required defaultValue={skill.title} />
          </label>
          <label>
            用途
            <textarea
              name="description"
              required
              defaultValue={skill.description}
            />
          </label>
          <label>
            使用示例
            <textarea name="example" required defaultValue={skill.example} />
          </label>
          {owner && (
            <label>
              维护人成员 ID
              <input name="maintainer_id" defaultValue={skill.maintainer_id} />
            </label>
          )}
          <button type="submit" disabled={busy}>
            保存介绍
          </button>
        </form>
      )}
      {publishing && (
        <PublishForm
          team={team}
          skill={skill}
          busy={busy}
          run={act}
          done={() => {
            setPublishing(false);
            void act(() => refresh());
          }}
          close={() => setPublishing(false)}
        />
      )}
    </section>
  );
}
