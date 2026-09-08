import { useEffect, useRef, useState } from "react";
import { request, segment, teamPath, type Member, type Team } from "./client";
import type { Runner } from "./operations";

export function invitationToken(value: string) {
  const trimmed = value.trim();
  if (/^https?:\/\//.test(trimmed)) {
    const token = new URL(trimmed).searchParams.get("invite");
    if (!token) throw new Error("邀请链接缺少令牌。");
    return token;
  }
  return trimmed;
}
export function TeamForms({
  run,
  done,
}: {
  run: Runner;
  done: (preferred?: string) => Promise<void>;
}) {
  const creation = useRef<{ name: string; key: string } | null>(null);
  const [token, setToken] = useState(
    () => new URL(window.location.href).searchParams.get("invite") ?? "",
  );
  return (
    <div className="team-form-grid">
      <form
        className="team-card"
        onSubmit={(e) => {
          e.preventDefault();
          const name = String(new FormData(e.currentTarget).get("name"));
          if (creation.current?.name !== name)
            creation.current = { name, key: crypto.randomUUID() };
          const key = creation.current.key;
          void run(async () => {
            await request("/v1/teams", {
              method: "POST",
              body: { name },
              idempotencyKey: key,
            });
            await done();
            if (creation.current?.key === key) creation.current = null;
          });
        }}
      >
        <h2>创建团队</h2>
        <label>
          团队名称
          <input
            name="name"
            required
            maxLength={100}
            placeholder="例如：产品研发组"
          />
        </label>
        <button className="primary" type="submit">
          创建空间
        </button>
      </form>
      <form
        className="team-card"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            const accepted = await request<{ team_id: string }>(
              "/v1/invitations/accept",
              {
                method: "POST",
                body: { token: invitationToken(token) },
              },
            );
            setToken("");
            const url = new URL(window.location.href);
            url.searchParams.delete("invite");
            window.history.replaceState(null, "", url);
            await done(accepted.team_id);
          });
        }}
      >
        <h2>已有邀请？</h2>
        <label>
          邀请链接或令牌
          <input
            required
            autoComplete="off"
            value={token}
            onChange={(e) => setToken(e.target.value)}
          />
        </label>
        <p>请使用接收邀请的邮箱登录。</p>
        <button type="submit">加入团队</button>
      </form>
    </div>
  );
}

export function TeamSettings({
  team,
  userId,
  run,
  done,
}: {
  team: Team;
  userId: string;
  run: Runner;
  done: (preferred?: string) => Promise<void>;
}) {
  const [members, setMembers] = useState<Member[]>([]);
  const [invites, setInvites] = useState<
    { id: string; url: string; email: string }[]
  >([]);
  const [transfer, setTransfer] = useState("");
  const [confirmLeave, setConfirmLeave] = useState(false);
  const [busy, setBusy] = useState(false);
  const owner = team.owner_user_id === userId;
  const act = (fn: () => Promise<void>) =>
    run(async () => {
      setBusy(true);
      try {
        await fn();
      } finally {
        setBusy(false);
      }
    });
  useEffect(() => {
    let alive = true;
    void run(async () => {
      const data = await request<{ members: Member[] }>(
        `${teamPath(team.id)}/members`,
      );
      if (alive) setMembers(data.members);
    });
    return () => {
      alive = false;
    };
  }, [team.id, run]);
  return (
    <section className="team-content">
      <span className="eyebrow">WORKSPACE</span>
      <h1>团队设置</h1>
      <p className="lead">
        {team.name} · {owner ? "你是团队所有者" : "你是团队成员"}
      </p>
      <TeamForms run={run} done={done} />
      <div className="team-card">
        <h2>成员与邀请</h2>
        {members.map((m) => (
          <div className="member-row" key={m.user_id}>
            <span>{m.email ?? m.user_id}</span>
            <small>
              {m.user_id === team.owner_user_id ? "所有者" : "成员"}
            </small>
            {owner && m.user_id !== userId && (
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void act(async () => {
                    await request(
                      `${teamPath(team.id)}/members/${segment(m.user_id)}`,
                      { method: "DELETE" },
                    );
                    setMembers((old) =>
                      old.filter((v) => v.user_id !== m.user_id),
                    );
                  })
                }
              >
                移除
              </button>
            )}
          </div>
        ))}
        {owner && (
          <>
            <form
              className="inline-form"
              onSubmit={(e) => {
                e.preventDefault();
                const email = String(
                  new FormData(e.currentTarget).get("email"),
                );
                void act(async () => {
                  const data = await request<{
                    invitation: { id: string; token: string };
                  }>(`${teamPath(team.id)}/invitations`, {
                    method: "POST",
                    body: { email },
                  });
                  const url = new URL("team.html", window.location.href);
                  url.searchParams.set("invite", data.invitation.token);
                  setInvites((old) => [
                    ...old,
                    { id: data.invitation.id, url: url.href, email },
                  ]);
                });
              }}
            >
              <label>
                邀请同事
                <input name="email" required type="email" />
              </label>
              <button type="submit" disabled={busy}>
                创建邀请
              </button>
            </form>
            {invites.map((invite) => (
              <div key={invite.id}>
                <p>{invite.email}</p>
                <label>
                  一次性邀请链接
                  <input value={invite.url} readOnly />
                </label>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      await navigator.clipboard.writeText(invite.url);
                    })
                  }
                >
                  复制邀请链接
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      await request(
                        `${teamPath(team.id)}/invitations/${segment(invite.id)}`,
                        { method: "DELETE" },
                      );
                      setInvites((old) =>
                        old.filter((v) => v.id !== invite.id),
                      );
                    })
                  }
                >
                  撤销邀请
                </button>
              </div>
            ))}
            <p className="subtle">
              这里只显示本次页面创建的邀请。请自行发送链接给同事。
            </p>
            <h3>转交团队所有权</h3>
            <label>
              新的所有者
              <select
                value={transfer}
                onChange={(e) => setTransfer(e.target.value)}
              >
                <option value="">选择现有成员</option>
                {members
                  .filter((m) => m.user_id !== userId)
                  .map((m) => (
                    <option key={m.user_id} value={m.user_id}>
                      {m.email ?? m.user_id}
                    </option>
                  ))}
              </select>
            </label>
            <p>确认后，你将成为普通成员。</p>
            <button
              type="button"
              disabled={busy || !transfer}
              onClick={() =>
                void act(async () => {
                  await request(`${teamPath(team.id)}/transfer-owner`, {
                    method: "POST",
                    body: { user_id: transfer },
                  });
                  setTransfer("");
                  await done();
                })
              }
            >
              确认转交所有权
            </button>
          </>
        )}
        {!owner && (
          <>
            <h3>退出团队</h3>
            <p>退出后无法访问云端内容，已安装文件仍在本机。</p>
            <label>
              <input
                type="checkbox"
                checked={confirmLeave}
                onChange={(e) => setConfirmLeave(e.target.checked)}
              />
              我确认退出此团队
            </label>
            <button
              type="button"
              disabled={busy || !confirmLeave}
              onClick={() =>
                void act(async () => {
                  await request(
                    `${teamPath(team.id)}/members/${segment(userId)}`,
                    { method: "DELETE" },
                  );
                  await done();
                })
              }
            >
              退出团队
            </button>
          </>
        )}
      </div>
    </section>
  );
}
