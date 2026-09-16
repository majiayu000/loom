import blueStyles from "./studio-blue.css?inline";
import darkStyles from "./studio.css?inline";
import methodBridge from "../assets/loom-method-bridge.png";
import paperSteps from "../assets/loom-paper-steps.png";
import { StudioWelcome } from "./StudioWelcome";
import { Updates } from "./Updates";
import { LocalActivity } from "./LocalActivity";
import { SkillDetail } from "./SkillDetail";
import { TeamForms, TeamSettings } from "./TeamSettings";
import { PublishForm } from "./PublishForm";
import { LocalSkills } from "./operations";
import { AskChat } from "./AskChat";
import { AutomationWorkbench } from "../automation/AutomationWorkbench";
import type { Envelope } from "./client";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  isDesktop,
  native,
  readConfig,
  request,
  requestOtp,
  saveConfig,
  signOut,
  teamPath,
  verifyOtp,
} from "./client";
import type { Config, Skill, Team } from "./client";

type Page = "team" | "local" | "updates" | "settings" | "activity" | "automation";
const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export function TeamApp() {
  const [theme, setTheme] = useState<"blue" | "dark">("dark");
  useEffect(() => {
    try {
      const saved = localStorage.getItem("loom-ui-theme");
      if (saved === "blue" || saved === "dark") setTheme(saved);
    } catch (err) {
      setError(`无法读取界面偏好：${message(err)}`);
    }
  }, []);
  const [page, setPage] = useState<Page>("team");
  const [user, setUser] = useState<{ id: string; email?: string } | null>(null);
  const [config, setConfig] = useState<Config>({
    cloud_api_url: "",
    auth_url: "",
    auth_public_key: "",
  });
  const [teams, setTeams] = useState<Team[]>([]);
  const [team, setTeam] = useState("");
  const [skills, setSkills] = useState<Skill[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [detail, setDetail] = useState<Skill | null>(null);
  const [detailRoot, setDetailRoot] = useState("");
  const [detailName, setDetailName] = useState<string | undefined>();
  const [search, setSearch] = useState("");
  const [archived, setArchived] = useState(false);
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
    const current = generation.current;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
    } catch (err) {
      if (current === generation.current) setError(message(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const loadTeams = useCallback(async (preferred?: string) => {
    const result = await request<{ teams: Team[] }>("/v1/me/teams");
    setTeams(result.teams);
    setTeam((current) =>
      result.teams.some((t) => t.id === (preferred ?? current))
        ? (preferred ?? current)
        : (result.teams[0]?.id ?? ""),
    );
  }, []);

  useEffect(() => {
    void readConfig()
      .then(async (value) => {
        setConfig(value);
        if (isDesktop()) {
          const identity = await native<{ id: string; email?: string } | null>(
            "current_user",
          );
          setUser(identity);
          if (identity) await loadTeams();
        }
      })
      .catch((err) => setError(message(err)));
  }, [loadTeams]);

  const loadSkills = useCallback(
    async (cursor?: string) => {
      if (!team) return;
      const current = generation.current;
      setLoading(true);
      try {
        const params = new URLSearchParams({ q: search, limit: "50" });
        if (archived) params.set("archived", "true");
        if (cursor) params.set("cursor", cursor);
        const result = await request<{
          skills: Skill[];
          next_cursor?: string | null;
        }>(`${teamPath(team)}/skills?${params}`);
        if (current !== generation.current) return;
        setSkills((old) =>
          cursor && page !== "updates"
            ? [...old, ...result.skills]
            : result.skills,
        );
        setNext(result.next_cursor ?? null);
      } catch (err) {
        if (current === generation.current) setError(message(err));
      } finally {
        if (current === generation.current) setLoading(false);
      }
    },
    [team, search, archived, page],
  );

  useEffect(() => {
    generation.current++;
    setSkills([]);
    setDetail(null);
    setError("");
    setNext(null);
    void loadSkills();
    return () => {
      generation.current++;
    };
  }, [loadSkills]);

  const selectSkill = (skill: Skill) => {
    setDetailRoot("");
    setDetailName(undefined);
    setDetail(skill);
  };

  const logout = () =>
    run(async () => {
      generation.current++;
      setUser(null);
      setTeams([]);
      setTeam("");
      setSkills([]);
      setDetail(null);
      setPublishing(false);
      await signOut();
    });

  return (
    <div className="loom-team-app" data-theme={theme}>
      <style>{theme === "blue" ? blueStyles : darkStyles}</style>
      <fieldset className="theme-switcher" aria-label="界面风格">
        {(["blue", "dark"] as const).map((choice) => (
          <button key={choice} type="button" aria-label={choice === "blue" ? "切换到蓝白版" : "切换到深色版"} aria-pressed={theme === choice} onClick={() => {
            try {
              localStorage.setItem("loom-ui-theme", choice);
              setTheme(choice);
            } catch (err) {
              setError(`无法保存界面偏好：${message(err)}`);
            }
          }}>{choice === "blue" ? "蓝白" : "深色"}</button>
        ))}
      </fieldset>
      <aside className="team-sidebar">
        <a className="team-brand" href="./team.html">
          <span className="loom-mark" aria-hidden="true">
            ▥
          </span>{" "}
          loom{theme === "blue" ? <span className="team-brand-tag">TEAM STUDIO</span> : <span className="brand-extension">.team</span>}
        </a>
        {theme === "dark" && <div className="nav-section-label">WORKSPACE</div>}
        <nav aria-label="主导航">
          {(
            [
              ["team", "团队技能", "01"],
              ["local", "本机技能", "02"],
              ["updates", "版本更新", "03"],
              ["activity", "本机活动", "04"],
              ["automation", "技能工作台", "05"],
              ["settings", "团队设置", "06"],
            ] as const
          ).map(([key, title, index]) => (
            <button
              type="button"
              key={key}
              className={page === key ? "active" : ""}
              aria-current={page === key ? "page" : undefined}
              onClick={() => {
                setPage(key);
                setDetail(null);
                setPublishing(false);
              }}
            >
              <span>{index}</span>
              {title}
              <span className="nav-arrow">↗</span>
            </button>
          ))}
        </nav>
        {theme === "dark" && <div className="sidebar-manifest"><span>SHARED KNOW-HOW</span><p>好方法，<br />值得团队共享。</p><small>技能在你的工具中运行。</small></div>}
        <div className="team-account">
          <span className="account-dot" />
          {user?.email ?? (user ? "已登录" : "尚未登录")}
          {user && (
            <button type="button" onClick={() => void logout()} disabled={busy}>
              退出
            </button>
          )}
        </div>
      </aside>
      <main className="team-main">
        <header className="team-topbar">
                  <div className="workspace-picker">
          <label htmlFor="team-picker">当前工作空间</label>
          <select
            id="team-picker"
            value={team}
            disabled={busy || !teams.length}
            onChange={(e) => {
              setTeam(e.target.value);
              setPublishing(false);
            }}
          >
            <option value="">选择团队</option>
            {teams.map((t) => (
              <option value={t.id} key={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
<span className="workspace-context">{page === "local" || page === "activity" || page === "automation" ? "ON THIS DEVICE" : "SHARED WORKSPACE"}</span>

        </header>
        {error && (
          <div role="alert" className="team-alert">
            <strong>操作未完成</strong>
            <span>{error}</span>
            <button
              type="button"
              onClick={() => setError("")}
              aria-label="关闭错误"
            >
              ×
            </button>
          </div>
        )}
        {notice && (
          <div role="status" className="team-notice">
            {notice}
          </div>
        )}
        {page === "local" ? (
          <LocalSkills run={run} />
        ) : page === "activity" ? (
          <LocalActivity />
        ) : page === "automation" ? (
          <AutomationWorkbench desktop unavailable={!isDesktop()} execute={(request, root) => native<Envelope>("automation_execute", { request, root: root ?? null })} />
        ) : !user ? (
          <section className="login-layout">
            <StudioWelcome blue={theme === "blue"} />
            <form
              className="team-card login-card"
              onSubmit={(e) => {
                e.preventDefault();
                void run(async () => {
                  if (!sent) {
                    await requestOtp(email);
                    setSent(true);
                    setNotice("验证码已发送，请查看邮箱。");
                  } else {
                    const identity = await verifyOtp(email, otp);
                    setUser(identity);
                    if (
                      new URL(window.location.href).searchParams.has("invite")
                    )
                      setPage("settings");
                    await loadTeams();
                  }
                });
              }}
            >
              <span className="eyebrow">YOUR NEXT GOOD IDEA STARTS HERE</span>
              <h2>一起，把经验<br />变成好方法。</h2>
              <h3 className="login-section-label">登录 / 注册团队账号</h3>
              <p>使用邮箱验证码登录。首次验证会自动创建账号，无需设置密码。</p>
              <label>
                工作邮箱
                <input
                  required
                  type="email"
                  autoComplete="email"
                  value={email}
                  onChange={(e) => {
                    setEmail(e.target.value);
                    setSent(false);
                  }}
                />
              </label>
              {sent && (
                <label>
                  邮箱验证码
                  <input
                    required
                    autoComplete="one-time-code"
                    inputMode="numeric"
                    value={otp}
                    onChange={(e) => setOtp(e.target.value)}
                  />
                </label>
              )}
              <button className="primary" disabled={busy} type="submit">
                {busy ? "请稍候…" : sent ? "验证并登录 →" : "发送验证码 →"}
              </button>
              {sent && (
                <button
                  type="button"
                  className="text-button"
                  disabled={busy}
                  onClick={() => setSent(false)}
                >
                  重新发送验证码
                </button>
              )}
              <details>
                <summary>服务连接设置</summary>
                <ConfigForm value={config} onChange={setConfig} />
                <button
                  disabled={busy}
                  type="button"
                  onClick={() =>
                    void run(async () => {
                      await saveConfig(config);
                      setNotice(
                        "服务设置已保存。网页登录凭证仅保存在本次会话内。",
                      );
                    })
                  }
                >
                  保存连接设置
                </button>
              </details>
              <small>
                {isDesktop()
                  ? "登录会话由桌面应用管理。"
                  : "网页登录状态仅保留在当前页面，刷新后需重新登录。"}
              </small>
            </form>
          </section>
        ) : !team ? (
          <section className="team-content">
            <span className="eyebrow">WELCOME TO LOOM</span>
            <h1>从一个团队开始。</h1>
            <p className="lead">创建空间，邀请同事，共享你们自己的技能。</p>
            <TeamForms run={run} done={loadTeams} />
          </section>
        ) : page === "settings" && selectedTeam ? (
          <TeamSettings
            key={team}
            team={selectedTeam}
            userId={user.id}
            run={run}
            done={loadTeams}
          />
        ) : detail ? (
          <SkillDetail
            key={`${team}:${detail.id}`}
            team={team}
            initial={detail}
            initialRoot={detailRoot}
            localName={detailName}
            owner={owner}
            userId={user.id}
            run={run}
            back={() => {
              setDetail(null);
              void loadSkills();
            }}
            notice={setNotice}
          />
        ) : (
          <section className="team-content">
            <div className="page-heading illustrated-heading">
              {theme === "blue" && <img className="workspace-art" src={page === "updates" ? paperSteps : methodBridge} alt="" aria-hidden="true" />}
              <div>
                <span className="eyebrow">
                  {page === "updates"
                    ? "KEEP IN STEP"
                    : "YOUR TEAM'S SHARED LIBRARY"}
                </span>
                <h1>
                  {page === "updates"
                    ? theme === "blue" ? "跟上团队的改进。" : <>每一次更新，<span className="heading-accent">都向前一步。</span></>
                    : theme === "blue" ? "团队的好方法，\n都在这里。" : <>团队的好方法，<span className="heading-accent">都在这里。</span></>}
                </h1>
                <p className="lead">
                  {page === "updates"
                    ? "查看最新发布，选择要更新的技能。"
                    : "发现同事分享的技能，带到你习惯的工具中。"}
                </p>
              </div>
              <button
                className="primary"
                type="button"
                onClick={() => setPublishing(true)}
              >
                ＋ 分享技能
              </button>
            </div>
            <label className="archive-toggle">
              <input
                type="checkbox"
                checked={archived}
                onChange={(e) => setArchived(e.target.checked)}
              />
              包含已归档技能
            </label>
            <div className="library-toolbar">
              <label className="search-label">
                <span aria-hidden="true">⌕</span>
                <input
                  aria-label="搜索团队技能"
                  placeholder="搜索名称、用途…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              </label>
              <button
                type="button"
                disabled={loading}
                onClick={() => void loadSkills()}
              >
                {loading ? "正在检查…" : "刷新目录 ↻"}
              </button>
            </div>
            {loading && !skills.length ? (
              <div role="status" className="empty-state">
                正在读取团队技能…
              </div>
            ) : !skills.length && !error ? (
              <div className="empty-state">
                <span className="paper-book" aria-hidden="true"><i /><i /><i /></span>
                <h2>
                  {search ? "没有找到匹配的技能" : "第一个好方法，由你分享。"}
                </h2>
                <p>
                  {search
                    ? "换一个名称或用途试试。"
                    : "选择一个你已经用过的 Skill，写下用途和一个使用示例。"}
                </p>
                {!search && (
                  <button type="button" onClick={() => setPublishing(true)}>
                    分享第一个技能
                  </button>
                )}
              </div>
            ) : page === "updates" ? (
              <Updates
                key={team}
                team={team}
                origin={config.cloud_api_url}
                skills={skills}
                run={run}
                open={(skill, root, localName) => {
                  setDetailRoot(root);
                  setDetailName(localName);
                  setDetail(skill);
                }}
              />
            ) : (
              <div className="skill-grid">
                {skills.map((s, i) => (
                  <button
                    className="skill-card"
                    type="button"
                    key={s.id}
                    onClick={() => void selectSkill(s)}
                  >
                    <span className="skill-card-top">
                      <span className={`skill-emblem emblem-${i % 3}`} aria-hidden="true">✧</span>
                      <span className="skill-number">
                        {String(i + 1).padStart(2, "0")}
                      </span>
                      <span>↗</span>
                    </span>
                    <h2>{s.title || s.slug}</h2>
                    <p>{s.description}</p>
                    <span className="skill-card-bottom">
                      <span className="skill-card-slug">{s.slug}</span>
                      <span>查看技能 →</span>
                    </span>
                  </button>
                ))}
              </div>
            )}
            {next && (
              <button
                type="button"
                disabled={loading}
                onClick={() => void loadSkills(next)}
              >
                {page === "updates" ? "下一页" : "加载更多"}
              </button>
            )}
            {publishing && (
              <PublishForm
                team={team}
                busy={busy}
                run={run}
                done={() => {
                  setPublishing(false);
                  void loadSkills();
                }}
                close={() => setPublishing(false)}
              />
            )}
          </section>
        )}
      </main>
      <AskChat
        skills={skills}
        signedIn={Boolean(user)}
        onOpenSkill={(skill) => {
          const found = skills.find((item) => item.id === skill.id);
          if (!found) {
            setError("打不开这个技能：目录里已经找不到它。");
            return;
          }
          setPage("team");
          setPublishing(false);
          selectSkill(found);
        }}
      />
    </div>
  );
}

function ConfigForm({
  value,
  onChange,
}: {
  value: Config;
  onChange: (v: Config) => void;
}) {
  return (
    <div className="config-fields">
      {(
        [
          ["cloud_api_url", "团队 API 地址"],
          ["auth_url", "Supabase 项目地址"],
          ["auth_public_key", "公开客户端密钥（非服务端密钥）"],
        ] as const
      ).map(([key, title]) => (
        <label key={key}>
          {title}
          <input
            type={key.endsWith("url") ? "url" : "text"}
            value={value[key]}
            onChange={(e) => onChange({ ...value, [key]: e.target.value })}
          />
        </label>
      ))}
    </div>
  );
}
