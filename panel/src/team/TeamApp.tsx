import { useCallback, useEffect, useRef, useState } from "react";
import { isDesktop, native, readConfig, request, requestOtp, requireSuccess, saveConfig, segment, signOut, teamPath, verifyOtp } from "./client";
import type { Config, Envelope, Member, Skill, Team, Version } from "./client";

type Page = "team" | "local" | "updates" | "settings";
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const date = (value: string) => new Date(value).toLocaleDateString("zh-CN");

export function TeamApp() {
  const [page, setPage] = useState<Page>("team");
  const [user, setUser] = useState<{ id: string; email?: string } | null>(null);
  const [config, setConfig] = useState<Config>({ cloud_api_url: "", auth_url: "", auth_public_key: "" });
  const [teams, setTeams] = useState<Team[]>([]);
  const [team, setTeam] = useState("");
  const [skills, setSkills] = useState<Skill[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [detail, setDetail] = useState<Skill | null>(null);
  const [versions, setVersions] = useState<Version[]>([]);
  const [members, setMembers] = useState<Member[]>([]);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [email, setEmail] = useState("");
  const [otp, setOtp] = useState("");
  const [sent, setSent] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const generation = useRef(0);
  const selectedTeam = teams.find((item) => item.id === team);
  const owner = selectedTeam?.owner_user_id === user?.id;

  const run = useCallback(async (action: () => Promise<void>) => {
    setBusy(true); setError(""); setNotice("");
    try { await action(); } catch (err) { setError(message(err)); }
    finally { setBusy(false); }
  }, []);

  const loadTeams = useCallback(async () => {
    const result = await request<{ teams: Team[] }>("/v1/me/teams");
    setTeams(result.teams); setTeam((current) => result.teams.some((t) => t.id === current) ? current : result.teams[0]?.id ?? "");
  }, []);

  useEffect(() => { readConfig().then(setConfig).catch((err) => setError(message(err))); }, []);

  const loadSkills = useCallback(async (cursor?: string) => {
    if (!team) return;
    const current = generation.current;
    setLoading(true);
    try {
      const params = new URLSearchParams({ q: search, limit: "50" });
      if (cursor) params.set("cursor", cursor);
      const result = await request<{ skills: Skill[]; next_cursor?: string | null }>(`${teamPath(team)}/skills?${params}`);
      if (current !== generation.current) return;
      setSkills((old) => cursor ? [...old, ...result.skills] : result.skills); setNext(result.next_cursor ?? null);
    } catch (err) { if (current === generation.current) setError(message(err)); }
    finally { if (current === generation.current) setLoading(false); }
  }, [team, search]);

  useEffect(() => {
    generation.current++; setSkills([]); setDetail(null); setVersions([]); setMembers([]); setError(""); setNext(null);
    void loadSkills();
    return () => { generation.current++; };
  }, [loadSkills]);

  const selectSkill = (skill: Skill) => run(async () => {
    const current = generation.current;
    const data = await request<{ versions: Version[] }>(`${teamPath(team)}/skills/${segment(skill.id)}/versions`);
    if (current !== generation.current) return;
    setDetail(skill); setVersions(data.versions);
  });

  const logout = () => run(async () => {
    generation.current++;
    setUser(null); setTeams([]); setTeam(""); setSkills([]); setDetail(null); setVersions([]); setMembers([]); setPublishing(false);
    await signOut();
  });

  return <div className="loom-team-app">
    <aside className="team-sidebar">
      <a className="team-brand" href="./team.html"><span className="loom-mark" aria-hidden="true">▥</span> loom<span className="team-brand-tag">TEAMSPACE</span></a>
      <div className="workspace-picker"><label htmlFor="team-picker">当前工作空间</label><select id="team-picker" value={team} disabled={busy || !teams.length} onChange={(e) => { setTeam(e.target.value); setPublishing(false); }}><option value="">选择团队</option>{teams.map((t) => <option value={t.id} key={t.id}>{t.name}</option>)}</select></div>
      <nav aria-label="主导航">{([["team", "团队技能", "01"], ["local", "本机技能", "02"], ["updates", "版本更新", "03"], ["settings", "团队设置", "04"]] as const).map(([key, title, index]) => <button type="button" key={key} className={page === key ? "active" : ""} onClick={() => { setPage(key); setDetail(null); setPublishing(false); }}><span>{index}</span>{title}<span className="nav-arrow">↗</span></button>)}</nav>
      <div className="sidebar-note"><span className="eyebrow">SHARED KNOW-HOW</span><p>好用的方法，<br />值得整个团队拥有。</p><small>技能在你的工具中运行。<br />文件由你决定何时安装。</small></div>
      <div className="team-account"><span className="account-dot" />{user?.email ?? (user ? "已登录" : "尚未登录")}{user && <button type="button" onClick={() => void logout()} disabled={busy}>退出</button>}</div>
    </aside>
    <main className="team-main">
      <header className="team-topbar"><span>{selectedTeam?.name ?? "你的团队工作空间"}</span><span className="surface-tag">{isDesktop() ? "DESKTOP" : "WEB"}</span></header>
      {error && <div role="alert" className="team-alert"><strong>操作未完成</strong><span>{error}</span><button type="button" onClick={() => setError("")} aria-label="关闭错误">×</button></div>}
      {notice && <div role="status" className="team-notice">{notice}</div>}
      {page === "local" ? <LocalSkills run={run} /> : !user ? <section className="login-layout"><div><span className="eyebrow">A LIBRARY BUILT BY YOUR TEAM</span><h1>让好方法，<br />成为共同习惯。</h1><p className="lead">分享你已经用顺手的技能。<br />同事在自己的 AI 工具中，接着用。</p><div className="onboarding-steps"><span>01 分享方法</span><span>02 安装技能</span><span>03 一起改进</span></div></div><form className="team-card login-card" onSubmit={(e) => { e.preventDefault(); void run(async () => { if (!sent) { await requestOtp(email); setSent(true); setNotice("验证码已发送，请查看邮箱。"); } else { const identity = await verifyOtp(email, otp); setUser(identity); await loadTeams(); } }); }}><h2>进入团队空间</h2><p>使用邮箱验证码登录，无需共享模型账户。</p><label>工作邮箱<input required type="email" autoComplete="email" value={email} onChange={(e) => { setEmail(e.target.value); setSent(false); }} /></label>{sent && <label>邮箱验证码<input required autoComplete="one-time-code" inputMode="numeric" value={otp} onChange={(e) => setOtp(e.target.value)} /></label>}<button className="primary" disabled={busy} type="submit">{busy ? "请稍候…" : sent ? "验证并登录 →" : "发送验证码 →"}</button>{sent && <button type="button" className="text-button" disabled={busy} onClick={() => setSent(false)}>重新发送验证码</button>}<details><summary>服务连接设置</summary><ConfigForm value={config} onChange={setConfig} /><button disabled={busy} type="button" onClick={() => void run(async () => { await saveConfig(config); setNotice("服务设置已保存。网页登录凭证仅保存在本次会话内。"); })}>保存连接设置</button></details><small>网页登录状态仅保留在当前页面，刷新后需重新登录。</small></form></section>
      : !team ? <section className="team-content"><span className="eyebrow">WELCOME TO LOOM</span><h1>从一个团队开始。</h1><p className="lead">创建空间，邀请同事，共享你们自己的技能。</p><TeamForms run={run} done={loadTeams} /></section>
      : page === "settings" ? <section className="team-content"><span className="eyebrow">WORKSPACE</span><h1>团队设置</h1><p className="lead">{selectedTeam?.name} · {owner ? "你是团队所有者" : "你是团队成员"}</p><TeamForms run={run} done={loadTeams} /><div className="team-card"><h2>成员与邀请</h2><button type="button" disabled={busy} onClick={() => void run(async () => { const data = await request<{ members: Member[] }>(`${teamPath(team)}/members`); setMembers(data.members); })}>查看成员</button>{members.map((m) => <div className="member-row" key={m.user_id}><span>{m.email ?? m.user_id}</span><small>{m.user_id === selectedTeam?.owner_user_id ? "所有者" : "成员"}</small>{owner && m.user_id !== user.id && <button type="button" onClick={() => void run(async () => { await request(`${teamPath(team)}/members/${segment(m.user_id)}`, { method: "DELETE" }); setMembers((old) => old.filter((v) => v.user_id !== m.user_id)); })}>移除</button>}</div>)}{owner && <form className="inline-form" onSubmit={(e) => { e.preventDefault(); const input = new FormData(e.currentTarget); void run(async () => { const data = await request<{ invite_url?: string; token?: string; invitation?: { token?: string } }>(`${teamPath(team)}/invitations`, { method: "POST", body: { email: input.get("email") } }); setNotice(`邀请已创建，请自行发送给同事：${data.invite_url ?? data.token ?? data.invitation?.token ?? "请从邀请详情获取链接"}`); }); }}><label>邀请同事<input type="email" name="email" required placeholder="colleague@company.com" /></label><button type="submit" disabled={busy}>创建邀请</button></form>}</div></section>
      : detail ? <section className="team-content"><button type="button" className="text-button" onClick={() => setDetail(null)}>← 返回技能库</button><div className="page-heading"><div><span className="eyebrow">TEAM SKILL</span><h1>{detail.title || detail.slug}</h1><p className="lead">{detail.description}</p></div><span className="skill-glyph">↗</span></div><div className="detail-grid"><div><div className="team-card"><span className="eyebrow">TRY THIS</span><h2>试着这样使用</h2><pre className="example">{detail.example || "作者尚未填写示例。"}</pre><button type="button" disabled={!detail.example} onClick={() => void run(async () => { await navigator.clipboard.writeText(detail.example); setNotice("使用示例已复制。请在安装后新建 Agent 会话，再粘贴使用。"); })}>复制使用示例</button></div><h2 className="section-title">版本历史 <span>{versions.length}</span></h2>{versions.map((v) => <div className="version-row" key={v.id}><div><strong>v{v.version}</strong>{v.id === detail.recommended_version_id && <span className="badge">推荐版本</span>}<p>{v.release_notes || "暂无变更说明"}</p><small>{date(v.created_at)}</small></div><span className="checksum" title={v.sha256}>{v.sha256.slice(0, 10)}</span></div>)}</div><div><InstallSkill team={team} skill={detail} versions={versions} run={run} /><div className="detail-meta"><span>由团队维护</span><strong>{detail.slug}</strong><p>安装成功后，仍需检查依赖和 Agent 可见性。</p></div>{(owner || detail.maintainer_id === user.id) && <button type="button" onClick={() => setPublishing(true)}>发布新版本</button>}</div></div>{publishing && <PublishForm team={team} skill={detail} busy={busy} run={run} done={() => { setPublishing(false); void selectSkill(detail); }} close={() => setPublishing(false)} />}</section>
      : <section className="team-content"><div className="page-heading"><div><span className="eyebrow">{page === "updates" ? "KEEP IN STEP" : "YOUR TEAM'S SHARED LIBRARY"}</span><h1>{page === "updates" ? "跟上团队的改进。" : "团队的好方法，\n都在这里。"}</h1><p className="lead">{page === "updates" ? "查看最新发布，选择要更新的技能。" : "发现同事分享的技能，带到你习惯的工具中。"}</p></div><button className="primary" type="button" onClick={() => setPublishing(true)}>＋ 分享技能</button></div><div className="library-toolbar"><label className="search-label"><span aria-hidden="true">⌕</span><input aria-label="搜索团队技能" placeholder="搜索名称、用途…" value={search} onChange={(e) => setSearch(e.target.value)} /></label><button type="button" disabled={loading} onClick={() => void loadSkills()}>{loading ? "正在检查…" : "刷新目录 ↻"}</button></div>{page === "updates" && <p className="subtle">这里显示云端目录，不代表设备安装状态。选择技能查看版本；本地修改不会被自动覆盖。</p>}{loading && !skills.length ? <div role="status" className="empty-state">正在读取团队技能…</div> : !skills.length && !error ? <div className="empty-state"><span className="empty-symbol">＋</span><h2>{search ? "没有找到匹配的技能" : "第一个好方法，由你分享。"}</h2><p>{search ? "换一个名称或用途试试。" : "选择一个你已经用过的 Skill，写下用途和一个使用示例。"}</p>{!search && <button type="button" onClick={() => setPublishing(true)}>分享第一个技能</button>}</div> : <div className="skill-grid">{skills.map((s, i) => <button className="skill-card" type="button" key={s.id} onClick={() => void selectSkill(s)}><span className="skill-card-top"><span className="skill-number">{String(i + 1).padStart(2, "0")}</span><span>↗</span></span><h2>{s.title || s.slug}</h2><p>{s.description}</p><span className="skill-card-bottom"><span>{s.slug}</span><span>查看技能 →</span></span></button>)}</div>}{next && <button type="button" disabled={loading} onClick={() => void loadSkills(next)}>加载更多</button>}{publishing && <PublishForm team={team} busy={busy} run={run} done={() => { setPublishing(false); void loadSkills(); }} close={() => setPublishing(false)} />}</section>}
    </main>
  </div>;
}

type Runner = (action: () => Promise<void>) => Promise<void>;
function ConfigForm({ value, onChange }: { value: Config; onChange: (v: Config) => void }) {
  return <div className="config-fields">{([["cloud_api_url", "团队 API 地址"], ["auth_url", "Supabase 项目地址"], ["auth_public_key", "公开客户端密钥（非服务端密钥）"]] as const).map(([key, title]) => <label key={key}>{title}<input type={key.endsWith("url") ? "url" : "text"} value={value[key]} onChange={(e) => onChange({ ...value, [key]: e.target.value })} /></label>)}</div>;
}

function TeamForms({ run, done }: { run: Runner; done: () => Promise<void> }) {
  return <div className="team-form-grid"><form className="team-card" onSubmit={(e) => { e.preventDefault(); const name = new FormData(e.currentTarget).get("name"); void run(async () => { await request("/v1/teams", { method: "POST", body: { name } }); await done(); }); }}><h2>创建团队</h2><label>团队名称<input name="name" required maxLength={100} placeholder="例如：产品研发组" /></label><button className="primary" type="submit">创建空间</button></form><form className="team-card" onSubmit={(e) => { e.preventDefault(); const token = String(new FormData(e.currentTarget).get("token")); void run(async () => { await request("/v1/invitations/accept", { method: "POST", body: { token } }); await done(); }); }}><h2>已有邀请？</h2><label>邀请令牌<input name="token" required autoComplete="off" /></label><p>请使用接收邀请的邮箱登录。</p><button type="submit">加入团队</button></form></div>;
}

function PublishForm({ team, skill, busy, run, done, close }: { team: string; skill?: Skill; busy: boolean; run: Runner; done: () => void; close: () => void }) {
  return <div className="publish-panel team-card"><div className="form-heading"><h2>{skill ? "发布新版本" : "分享一个好方法"}</h2><button type="button" disabled={busy} onClick={close} aria-label="关闭发布表单">×</button></div><form onSubmit={(e) => { e.preventDefault(); const values = new FormData(e.currentTarget); void run(async () => { const metadata = { slug: values.get("slug"), title: values.get("title"), description: values.get("description"), example: values.get("example"), version: values.get("version"), release_notes: values.get("release_notes"), expected_recommended_version_id: skill?.recommended_version_id ?? null }; const body = new FormData(); body.append("metadata", JSON.stringify(metadata)); const artifact = values.get("artifact"); if (!(artifact instanceof File) || !artifact.size) throw new Error("请选择完整的 .tar.gz Skill 包。"); if (isDesktop()) throw new Error("桌面文件发布入口正在接入。请使用团队网页上传完整 Skill 包。"); body.append("artifact", artifact); await request(`${teamPath(team)}/skills${skill ? `/${segment(skill.id)}/versions` : ""}`, { method: "POST", body }); done(); }); }}><div className="team-form-grid"><label>标识名称<input required name="slug" defaultValue={skill?.slug} readOnly={!!skill} pattern="[a-z0-9][a-z0-9-]*" placeholder="code-review" /></label><label>显示名称<input required name="title" defaultValue={skill?.title} placeholder="代码审查" /></label></div><label>它解决什么问题？<textarea required name="description" defaultValue={skill?.description} rows={2} /></label><label>同事可以怎样使用？<textarea required name="example" defaultValue={skill?.example} rows={2} placeholder="使用这个 Skill，检查当前分支的修改…" /></label><div className="team-form-grid"><label>版本号<input required name="version" defaultValue={skill ? "" : "0.1.0"} placeholder="1.2.0" /></label><label>完整 Skill 包<input required type="file" name="artifact" accept=".gz,.tgz" /></label></div><label>本次变更<textarea name="release_notes" required rows={2} /></label><p className="subtle">包根目录须包含 SKILL.md；请检查文件清单，移除凭证、.env 与 .git。上传不会执行脚本。</p><button className="primary" disabled={busy} type="submit">{busy ? "正在发布…" : "发布到团队"}</button></form></div>;
}

function InstallSkill({ team, skill, versions, run }: { team: string; skill: Skill; versions: Version[]; run: Runner }) {
  const [agent, setAgent] = useState("codex");
  const [workspace, setWorkspace] = useState("");
  const [version, setVersion] = useState(skill.recommended_version_id ?? versions[0]?.id ?? "");
  const [result, setResult] = useState<unknown>(null);
  return <div className="team-card install-card"><h2>安装到我的工具</h2><label>版本<select value={version} onChange={(e) => setVersion(e.target.value)}>{versions.map((v) => <option key={v.id} value={v.id}>v{v.version}{v.id === skill.recommended_version_id ? " · 推荐" : ""}</option>)}</select></label><label>目标工具<select value={agent} onChange={(e) => setAgent(e.target.value)}><option value="codex">Codex</option><option value="claude">Claude Code</option><option value="cursor">Cursor</option></select></label>{isDesktop() ? <><label>项目目录<input value={workspace} readOnly placeholder="选择要安装的项目" /></label><button type="button" onClick={() => void run(async () => { const path = await native<string | null>("choose_directory"); if (path) setWorkspace(path); })}>选择项目</button><button className="primary" type="button" disabled={!workspace || !version} onClick={() => void run(async () => { const value = await native<Envelope>("preview_team_install", { team, skill: skill.id, version, name: skill.slug, agent, workspace }); setResult(requireSuccess(value)); })}>预览安装</button></> : <p className="subtle">本机安装由桌面 App 完成。打开 Loom 后，在同一团队选择此技能和版本。</p>}{result !== null && <pre className="operation-result">{JSON.stringify(result, null, 2)}</pre>}</div>;
}

function LocalSkills({ run }: { run: Runner }) {
  const [root, setRoot] = useState("");
  const [result, setResult] = useState<Record<string, unknown> | null>(null);
  return <section className="team-content"><span className="eyebrow">ON YOUR MACHINE</span><h1>你的技能，<br />留在你的电脑上。</h1><p className="lead">检查本机技能，然后决定哪些值得分享。</p>{isDesktop() ? <div className="team-card"><label>Registry 目录（留空使用 Loom 默认目录）<input value={root} onChange={(e) => setRoot(e.target.value)} placeholder="使用默认 registry" /></label><button className="primary" type="button" onClick={() => void run(async () => { setResult(requireSuccess(await native<Envelope>("local_skills", { root: root || null }))); })}>读取本机技能</button>{result && <pre className="operation-result">{JSON.stringify(result, null, 2)}</pre>}</div> : <div className="empty-state"><span className="empty-symbol">⌘</span><h2>在桌面 App 中管理本机技能</h2><p>网页可以浏览团队内容，本机文件由桌面 App 读取和管理。</p></div>}</section>;
}
