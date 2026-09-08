# 桌面与团队服务实施验收

日期：2026-09-08。分支：`codex/desktop-cloud`。原始基线：`f3d81f1`。

## 交付范围

- `desktop/` 是 Tauri 桌面应用，内置同一 checkout 编译的 CLI；本地操作通过固定具名 IPC 命令执行。
- `panel/team.html` 提供团队目录、发布预览、版本历史、文件预览、成员管理、本地仓库导入与项目激活。
- `cloud/` 提供独立 Postgres 团队服务、私有不可变版本、邮箱邀请、维护权、推荐版本和条件更新。
- 团队包保留服务、团队、技能和版本身份。首次导入与激活分开；更新通过持久计划和事务检查本地修改，覆盖现有绑定目标。

没有部署远程服务、创建收费账户、保存真实 API 密钥、推送分支或合并远端 PR。原有 `landing/` 和两份 remem 审计文件保持原样。

## 验证方式

运行日志位于本 checkout 的 `.git/codex/threads/desktop-cloud/`；线程汇总位于 `.git/codex/threads/run-log.jsonl`。这些是本机证据，不随 Git 发布。

| 层 | 验证与证据 |
| --- | --- |
| Cloud | 临时独立 PostgreSQL，5 项测试通过；包括迁移、邀请、租户隔离、并发发布、不可变内容、条件推荐、真实签名 JWT 与密钥轮换。`cloud-tests.log` |
| Auth hook | 临时数据库模拟 Supabase Auth 表与角色，验证已确认邮箱、未确认邮箱、邮箱不匹配和函数执行权限。`hook-check.log`。不等于真实托管 OTP 验收 |
| Native | 10 项测试通过；包括路径/参数边界、包摘要、链接和 FIFO 拒绝、非 UTF-8 名称拒绝。`desktop-tests.log` |
| Frontend | 全量前端 217 项测试通过，团队部分 24 项；TypeScript 检查、lint 与生产构建通过。全库 lint 有 200 条既有警告，团队新增目录单独检查无警告。`frontend-final.log` |
| Browser | 登录页与详情页在桌面和窄屏检查；详情使用明确标注的测试数据，不能当作真实云端账户联调证据 |
| Engine | 团队安装 11 项、命令契约 32 项；inspect 23、provenance 12、workspace init 18、convergence 160、input review 16、org policy 3、provider 2 项回归通过；新增 archive/snapshot/cleanup 检查通过。工作线程 lint、fmt、模块上限通过，35 条既有接近阈值提示。整合输出 `engine-final.log`，工作线程分组日志 `worker-engine-*.log` |

原生文件读取审查发现的链接/FIFO 竞态和有损文件名问题已修复，并经只读复核关闭。云端审查发现的条件更新 header、JWT nbf 和密钥刷新问题已修复。一次非 UTF-8 文件测试受到 APFS 文件名限制而失败，改为直接测试实际收集入口使用的 UTF-8 边界，保留拒绝断言。

## 运行与发布边界

桌面构建见 [desktop/README.md](../../desktop/README.md)，服务配置见 [cloud/README.md](../../cloud/README.md)。首批产物为 Apple Silicon 未签名本地调试 App，尚未签名、公证或分发。团队服务需自行配置独立数据库、私有持久卷、TLS 和 Auth。

当前使用邮箱 OTP，Supabase 要配置已验证邮箱 hook 与发送数字验证码的邮件模板。网页令牌仅在内存，刷新后重新登录；桌面刷新凭证由系统凭证库保存。

尚未验收：真实托管 OTP/Keychain 全链路、干净机器安装、Windows、各 Agent 的真实新会话加载、备份恢复演练、限流部署及三团队内测。归档技能仅允许预览恢复已有同来源安装，原生检查本机身份并要求冻结计划代表更新；首次安装被拒绝。手动选择历史版本会建立固定版本计划。源码编译及模拟浏览器数据不替代上述验收。

认证、凭证和进程调用在公开发布前仍需人工审阅。新增 CI 工作流尚未推送，不能宣称远端 CI 已通过。

整合构建发现 `build.rs` 未复制新增 `team.html`，已补齐复制及重新构建触发。当前主目录有 Git 忽略的旧 `skills/loom/SKILL.md`，使严格命令 inventory 检查发现未登记的本机命令面；保留此用户本地文件，最终契约验收使用 `/tmp/loom-team-final-20260908` 干净 checkout，不修改检查规则。
