# Loom Cloud

独立 Rust/Axum API，Postgres 保存租户与不可变版本。它与 CLI 本地 `/api/v1` 服务分开，不读取或修改本机 registry。已实现团队、邀请、维护权、归档、版本发布/历史/文件预览/私有下载和条件推荐更新。

## 部署决策

当前部署使用**单实例 API + Postgres + 私有持久磁盘卷**。`LOOM_ARTIFACT_DIR` 必须是绝对路径，仅服务账号可访问（Unix 启动设置目录 0700），不能由 nginx/static 服务暴露。对象为服务端随机 UUID 文件，使用 `create_new`、fsync 后才提交数据库引用；失败孤立对象不能通过 API 枚举或下载。这是设计中私有对象存储的本地服务器实现，并非 Supabase Storage 集成；多实例部署需共享同一可靠持久文件系统。数据库和文件卷必须一起备份、恢复并校验摘要。

部署需配置以下环境变量；仓库不提供真实凭证，也不创建 `.env`。

| 变量 | 用途 |
| --- | --- |
| `DATABASE_URL` | 独立 PostgreSQL 数据库；服务账号负责迁移 |
| `LOOM_BIND` | 明确的监听地址与端口，由部署者分配 |
| `LOOM_ARTIFACT_DIR` | 私有持久卷绝对路径 |
| `LOOM_AUTH_ISSUER` | JWT 精确 issuer |
| `LOOM_AUTH_AUDIENCE` | JWT audience |
| `LOOM_AUTH_JWKS_URL` | HTTPS JWKS 地址（本机回环 IP 可使用 HTTP）；启动加载，未知 kid 与 5 分钟 TTL 触发刷新 |
| `LOOM_CORS_ORIGINS` | 逗号分隔的精确网页 origin，禁止 `*` |

运行 `cargo run --manifest-path cloud/Cargo.toml --release`。启动自动应用迁移；`GET /v1/health` 检查数据库连通性。生产入口必须提供 TLS。Auth 必须使用 RS256 或 ES256 JWT，验证签名、kid、issuer、audience、exp、sub；不存在开发登录或可信身份头。验证 `nbf` 并禁用时间宽限；JWKS 仅从配置地址读取，远程必须 HTTPS，本机回环 IP 允许 HTTP，禁止重定向，超时 10 秒、最大 1 MiB。未知 kid 触发刷新，互斥锁合并并发请求；全局至少间隔 30 秒，缓存最长 5 分钟。刷新失败明确 503，过期缓存不继续授权；冷却期间新轮换 key 可能需最多 30 秒后重试。数据库不得通过 Supabase Data API 对客户端开放；迁移撤销 PUBLIC 表权限，部署者还需撤销其平台上 anon/authenticated 等显式角色授权。不要向浏览器发放数据库凭证。

邮箱邀请要求 JWT 顶层 `email_verified: true` 与 `email`。使用 Supabase 时，在 Auth 项目执行 [supabase-email-hook.sql](supabase-email-hook.sql)，并在 Authentication → Hooks 启用 `public.loom_access_token_hook`；如已有 hook，应把已确认邮箱判断合入已有函数。此脚本需部署者审阅后手动执行，不属于 Loom 自动迁移。它只读取 `auth.users.email_confirmed_at` 和实际邮箱，不信任用户 metadata，并保留原有 claims。配置完成后重新登录取得新 JWT；未配置时接受邀请明确返回 403。接入方式依据 [Supabase Custom Access Token Hook](https://supabase.com/docs/guides/auth/auth-hooks/custom-access-token-hook) 与 [权限说明](https://supabase.com/docs/guides/auth/auth-hooks)。API 不保存浏览器会话。

每日运行同一二进制 `loom-cloud --gc`（仅需要数据库和存储环境变量）清理超过 24 小时且两次确认未引用的 UUID 对象和过期幂等记录。原始 artifact key 不返回给客户端。备份恢复演练、真实 Auth 账户、请求限流和签名生产部署仍需上线前验收；当前未声称生产上线。

## HTTP 契约

JSON 成功 `{data,request_id}`，错误 `{error:{message},request_id}`。授权为 `Authorization: Bearer …`。下载返回 `application/gzip` 原始字节且 `Cache-Control: private, no-store`。当前小包上传/下载有界缓冲（10 MiB 压缩 / 50 MiB 解包 / 1000 文件 / 2000 总条目），并非零缓冲流式传输。拒绝链接、设备、绝对/父级路径、Windows 分隔符、重复路径、`.git` 和 `.env*`；根目录必须有普通 `SKILL.md`。此校验不代表凭证扫描或安全认证。

- `GET /v1/me/teams` → `{teams:[{id,name,owner_user_id}]}`
- `POST /v1/teams`，`{name}`，必须带 `Idempotency-Key` → `{team}`。
- `GET /v1/teams/{team}/members` → `{members}`，成员含 `user_id,team_id,joined_at`。
- `POST /v1/teams/{team}/invitations`，`{email}` → `{invitation:{id,token,expires_in_seconds}}`。网页用自身 origin 拼接 token 接受链接；服务端只保存 token SHA-256。
- `DELETE /v1/teams/{team}/invitations/{id}` 撤销；`POST /v1/invitations/accept`，`{token}` → `{team_id}`。
- `DELETE /v1/teams/{team}/members/{user}` 移除/退出；`POST /v1/teams/{team}/transfer-owner`，`{user_id}` 转交所有权。
- `GET /v1/teams/{team}/skills?q=&cursor=&limit=&archived=true` → `{skills,next_cursor}`。默认不含归档；`archived=true` 包含归档。条目含 `recommended_version_id,recommended_version,maintainer_id,archived_at,revision`。默认 50，最大 100。
- `GET /v1/teams/{team}/skills/{skill}` → `{skill}`。
- `POST /v1/teams/{team}/skills` 和 `POST …/{skill}/versions` 使用 multipart `metadata` JSON 和 `artifact` tar.gz，必须带 `Idempotency-Key`。metadata 含 `slug,title,description,example,version,release_notes`；后续版本必须有 `expected_recommended_version_id`，也接受可选声明 `sha256`。后续发布仅用 version/release_notes/expected pointer；介绍通过 PATCH 修改。返回 `{skill_id,version}`，version 含 `id,version,sha256,release_notes,created_at,file_manifest,size_bytes,published_by`。
- `PATCH …/{skill}`，`If-Match: <revision>`，JSON 可含 `title,description,example,archived`；只有 owner 可以设置 `maintainer_id`。返回 `{skill}`。
- `PUT …/{skill}/recommendation`，`{version_id,expected_version_id}` → `{skill}`。
- `GET …/{skill}/versions?cursor=&limit=` → `{versions,next_cursor}`。
- `GET …/{skill}/versions/{version}` → `{version}`，授权后返回固定版本元数据及 SHA-256，不暴露内部存储 key。
- `GET …/{skill}/versions/{version}/files` → `{files}`；`…/file?path=` → `{file,text}`，超过 256 KiB 或二进制不返回文本；JSON 文本不得按 HTML 渲染。
- `GET …/{skill}/versions/{version}/artifact` 私有下载；归档仍允许查看历史、恢复既有安装，客户端负责阻止归档 Skill 新增安装。

Cursor 是最后一条资源 UUID，服务端在当前租户范围解析它对应的排序时间/id；无效或跨租户游标报错。目录按更新时间降序，历史按创建时间降序。

团队行锁串行化团队写入，发布事务重新验证即时成员和维护权；推荐比较、版本、事件在同事务。唯一 `(skill_id,version)` 防止重复标签；同摘要重试返回既有不可变版本，不改说明和发布者，不同摘要 409。幂等范围 actor/team/operation/key，24 小时保存请求摘要与结果，同键异内容 409。创建团队用零 UUID 作幂等团队作用域。成员删除把其 Skill 交给 owner，延迟外键保护 owner 成员及同 Skill 推荐引用。

## 验证

本机独立测试 PostgreSQL 已初始化在仓库 `.git/codex/local-team/pgdata`，只监听 `127.0.0.1:5572`，数据库 `loom_team_test`，用户 `loom_test`。使用 trust 认证，仅用于本机可丢弃测试数据，不用于真实团队数据。复跑测试：

```sh
LOOM_TEST_DATABASE_URL=postgresql://loom_test@127.0.0.1:5572/loom_team_test cargo test --manifest-path cloud/Cargo.toml --locked
```

在仓库根目录停止或重新启动此测试库：

```sh
pg_ctl -D .git/codex/local-team/pgdata stop
pg_ctl -D .git/codex/local-team/pgdata -l .git/codex/local-team/postgres.log -o '-h 127.0.0.1 -p 5572 -k /tmp' start
```

这只启动数据库。App 的邮箱登录 / 注册依赖 Supabase Auth，验证码请求已指定 `create_user: true`，首次验证自动创建账号；数据库集成测试不能替代真实邮箱注册和桌面登录联调。未配置 Auth/JWKS 时，生产 API 不会绕过身份校验启动。

`cargo fmt --manifest-path cloud/Cargo.toml -- --check`

设置 `LOOM_TEST_DATABASE_URL` 指向可丢弃 PostgreSQL 数据库，再运行 `cargo test --manifest-path cloud/Cargo.toml`。测试不会跳过缺失数据库：未配置直接失败。测试注入身份只存在 `#[cfg(test)]` 模块内，生产 HTTP 路由始终校验 JWT。覆盖归档边界、真实数据库迁移、邮箱绑定/重复邀请、移除后权限、owner 约束、创建幂等、并发发布推荐冲突、不可变重试、跨租户下载和篡改检测，以及签名 JWT 的未来 nbf 拒绝、JWKS 轮换/并发刷新限频与固定版本元数据隔离。测试库包含随机测试数据，应整库丢弃。

## 当前本地团队联调环境

- 团队 API：`http://127.0.0.1:5576`，使用原生 PG 的 `loom_team_local` 数据库，与自动测试库隔离。
- Supabase Auth 网关：`http://127.0.0.1:5574`，认证数据库位于专用 Docker volume。
- 测试收件箱：`http://127.0.0.1:5575`，所有验证码留在本机，不投递外部邮箱。
- Docker Compose 配置：`.git/codex/local-team/compose.json`；App 的三项连接配置：同目录 `app-config.json`。包含本地私有配置的文件不得提交或公开。

运行 `docker compose -f .git/codex/local-team/compose.json up -d` 可恢复认证和收件箱；团队 API 的启动环境见 `api-config.json`，日志 `api.log`，进程号 `api.pid`。PG 的启停见上文。团队服务和认证均使用真实 JWT 签名校验，未启用模拟登录。

测试账号 `owner@loom.test` 与 `member@loom.test` 已完成邮箱验证，已创建双成员的“Loom 本地测试团队”；每次登录仍需读取最新验证码。原生 App 已完成发送验证码、验证和进入团队的实测。测试证据 `smoke-result.json`，本机 JWKS 边界与原有后端共 6 项测试通过。认证配置参考 [Supabase 自托管 Auth Hook](https://supabase.com/docs/guides/self-hosting/self-hosted-auth-hooks)，收件箱使用 [Mailpit](https://mailpit.axllent.org/docs/api-v1/)。
