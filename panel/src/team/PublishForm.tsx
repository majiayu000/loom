import { useRef, useState } from "react";
import {
  isDesktop,
  native,
  request,
  segment,
  teamPath,
  type Skill,
} from "./client";
import type { Runner } from "./operations";
export function PublishForm({
  team,
  skill,
  busy,
  run,
  done,
  close,
}: {
  team: string;
  skill?: Skill;
  busy: boolean;
  run: Runner;
  done: () => void;
  close: () => void;
}) {
  const [source, setSource] = useState("");
  const [preview, setPreview] = useState<{
    files: { path: string; size: number }[];
    size_bytes: number;
    sha256: string;
  } | null>(null);
  const submission = useRef<{ fingerprint: string; key: string; file?: File }>({
    fingerprint: "",
    key: "",
  });
  return (
    <div className="publish-panel team-card">
      <div className="form-heading">
        <h2>{skill ? "发布新版本" : "分享一个好方法"}</h2>
        <button
          type="button"
          disabled={busy}
          onClick={close}
          aria-label="关闭发布表单"
        >
          ×
        </button>
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const values = new FormData(e.currentTarget);
          void run(async () => {
            const metadata = {
              slug: values.get("slug"),
              title: values.get("title"),
              description: values.get("description"),
              example: values.get("example"),
              version: values.get("version"),
              release_notes: values.get("release_notes"),
              expected_recommended_version_id:
                skill?.recommended_version_id ?? null,
            };
            if (isDesktop()) {
              if (!preview) throw new Error("请先选择目录并预览文件。");
              const fingerprint = JSON.stringify({
                metadata,
                source,
                sha256: preview.sha256,
              });
              if (submission.current.fingerprint !== fingerprint)
                submission.current = { fingerprint, key: crypto.randomUUID() };
              await native("publish_skill", {
                team,
                skill: skill?.id ?? null,
                source,
                metadata,
                expectedSha256: preview.sha256,
                idempotencyKey: submission.current.key,
              });
              done();
              return;
            }
            const body = new FormData();
            body.append("metadata", JSON.stringify(metadata));
            const artifact = values.get("artifact");
            if (!(artifact instanceof File) || !artifact.size)
              throw new Error("请选择完整的 .tar.gz Skill 包。");
            body.append("artifact", artifact);
            const fingerprint = JSON.stringify(metadata);
            if (
              submission.current.fingerprint !== fingerprint ||
              submission.current.file !== artifact
            )
              submission.current = {
                fingerprint,
                key: crypto.randomUUID(),
                file: artifact,
              };
            await request(
              `${teamPath(team)}/skills${skill ? `/${segment(skill.id)}/versions` : ""}`,
              { method: "POST", body, idempotencyKey: submission.current.key },
            );
            done();
          });
        }}
      >
        <div className="team-form-grid">
          <label>
            标识名称
            <input
              required
              name="slug"
              defaultValue={skill?.slug}
              readOnly={!!skill}
              pattern="[a-z0-9][a-z0-9-]*"
              placeholder="code-review"
            />
          </label>
          <label>
            显示名称
            <input
              required
              name="title"
              defaultValue={skill?.title}
              placeholder="代码审查"
            />
          </label>
        </div>
        <label>
          它解决什么问题？
          <textarea
            required
            name="description"
            defaultValue={skill?.description}
            rows={2}
          />
        </label>
        <label>
          同事可以怎样使用？
          <textarea
            required
            name="example"
            defaultValue={skill?.example}
            rows={2}
            placeholder="使用这个 Skill，检查当前分支的修改…"
          />
        </label>
        <div className="team-form-grid">
          <label>
            版本号
            <input
              required
              name="version"
              defaultValue={skill ? "" : "0.1.0"}
              placeholder="1.2.0"
            />
          </label>
          {isDesktop() ? (
            <div>
              <label>
                Skill 目录
                <input value={source} readOnly />
              </label>
              <button
                disabled={busy}
                type="button"
                onClick={() =>
                  void run(async () => {
                    const path = await native<string | null>(
                      "choose_directory",
                    );
                    if (!path) return;
                    setSource(path);
                    setPreview(null);
                    setPreview(
                      await native("preview_publish", { source: path }),
                    );
                  })
                }
              >
                选择目录并预览
              </button>
            </div>
          ) : (
            <label>
              完整 Skill 包
              <input required type="file" name="artifact" accept=".gz,.tgz" />
            </label>
          )}
        </div>
        <label>
          本次变更
          <textarea name="release_notes" required rows={2} />
        </label>
        <p className="subtle">
          包根目录须包含 SKILL.md；请检查文件清单，移除凭证、.env 与
          .git。上传不会执行脚本。
        </p>
        {preview && (
          <section aria-label="待发布文件">
            <p>
              {preview.files.length} 个文件 · 压缩后 {preview.size_bytes} 字节
            </p>
            <ul>
              {preview.files.map((file) => (
                <li key={file.path}>
                  {file.path} · {file.size} 字节
                </li>
              ))}
            </ul>
            <small className="checksum">SHA-256 {preview.sha256}</small>
          </section>
        )}
        <button
          className="primary"
          disabled={busy || (isDesktop() && !preview)}
          type="submit"
        >
          {busy ? "正在发布…" : "发布到团队"}
        </button>
      </form>
    </div>
  );
}
