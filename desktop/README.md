# Loom 桌面壳

独立 Tauri 2 crate，打包 `../panel/dist/team.html` 与版本绑定的 Loom CLI。前端仅在 `window.__TAURI_INTERNALS__` 存在时使用 `@tauri-apps/api/core` 的 `invoke`。浏览器网页没有本地操作权限；远程页面没有 Tauri capability。

## 构建

在仓库根目录运行已有 `make panel-build`（团队入口必须生成 `panel/dist/team.html`），随后：

```sh
./desktop/scripts/prepare-sidecar.sh
cargo check --manifest-path desktop/Cargo.toml --locked
cargo test --manifest-path desktop/Cargo.toml --locked
cd desktop
bunx @tauri-apps/cli@2.11.4 build --bundles app
```

开发者需 Rust、Git、Bun、平台 Tauri 构建依赖；构建命令使用固定版本 Tauri CLI，不要求全局安装。打包后的用户不需要 Rust/Bun；Git 仍需系统提供。`prepare-sidecar.sh [target-triple]` 编译同 checkout 的 CLI，不下载或调用用户 PATH 中的 loom。原生调用通过 Tauri sidecar 解析打包路径，参数为数组，前端没有通用 shell/文件读取入口。Windows bundle target 需根据发布平台选择；本次默认仅构建 macOS app，未做签名、公证或发布。

## Native bridge

所有名称为 `invoke` 命令名。输入对象字段按下表传入（下划线仅出现于 config 内容字段）；`?` 表示可省略。命令失败 reject 中文 string。CLI 命令直接返回原有 JSON envelope，UI 必须检查其 `ok`，不得把 Promise fulfilled 当安装成功。`root` 不传使用 CLI 默认 registry；显式 root/workspace/source 必须是已有绝对目录。不自动初始化 registry，不扫描任意磁盘。

| 命令 | 输入 | 返回/语义 |
| --- | --- | --- |
| `bootstrap` | 无 | `{git:{available,version? ,message?}}` |
| `choose_directory` / `choose_file` | 无 | 绝对路径或取消时 null |
| `local_skills` | `{root?}` | `loom --json skill list` |
| `inspect_skill` / `deps_skill` | `{root?,skill,agent?,workspace?}` | `skill inspect` / `skill deps` JSON |
| `visibility_skill` | `{root?,skill,agent,workspace?}` | `skill visibility` JSON |
| `diagnose_skill` | `{root?,skill}` | `skill diagnose`，只读诊断 |
| `history_skill` | `{root?,skill}` | `skill history --limit 30 --include-diff-stat` |
| `diff_skill` | `{root?,skill,from,to}` | `skill diff`，比较本机 Git 修订 |
| `local_operations` | `{root?,offset}` | `ops list --activity --limit 100 --offset`，本机活动分页 |
| `preview_install` / `apply_install` | `{root?,source,name}` | `skill install local:<source> --name <name>`，预览追加 `--dry-run` |
| `preview_activate` / `apply_activate` | `{root?,skill,agent,workspace?}` | `skill activate --agent`，有 workspace 为 project，否则 user；预览追加 `--dry-run` |
| `initialize_registry` | `{root?}` | 显式初始化本地 registry，不隐式覆盖已有目录 |
| `preview_publish` | `{source}` | 文件清单、压缩大小、SHA-256；拒绝链接、特殊文件、私密路径与非 UTF-8 文件名 |
| `publish_skill` | `{team,skill?,source,metadata,expectedSha256,idempotencyKey}` | 重新打包并匹配预览摘要后发布；skill 省略为首版 |
| `preview_team_install` | `{team,skill,version,name,root?,requestedRef?}` | 验证下载摘要后建立持久团队计划；requestedRef 为 recommended 或固定 version ID |
| `apply_plan` | `{root?,planId,planDigest,idempotencyKey}` | 执行冻结计划，保留引擎策略和冲突保护 |
| `get_cloud_config` | 无 | `{cloud_api_url,auth_url,auth_public_key}`，未设置时均空字符串 |
| `save_cloud_config` | `{config:{cloud_api_url,auth_url,auth_public_key}}` | 保存系统凭证库并清除之前的会话 |
| `request_otp` | `{email}` | Supabase `POST /auth/v1/otp`；成功 void |
| `verify_otp` | `{email,token}` | Supabase `POST /auth/v1/verify` type=email；再向 auth user 端点验证并返回 `{id,email}` |
| `current_user` | 无 | 通过刷新凭证及 auth user 端点返回 `{id,email}`；未登录 null |
| `cloud_request` | `{method,path,body?,idempotencyKey?,ifMatch?}` | `/v1/` 云端 API 成功 JSON；原生添加 bearer，不返回凭证 |
| `logout` | 无 | 删除持久刷新凭证与内存访问令牌，保留服务配置 |

`cloud_api_url` 为 API origin；`auth_url` 为 Supabase 项目 origin，均不带路径。远程必须 HTTPS，本机开发可用 HTTP localhost/127.0.0.1。请求禁用重定向，路径不能逃逸 `/v1/`。支持 GET/POST/PUT/PATCH/DELETE；JSON body 可选。访问令牌只在 Rust 内存，刷新凭证与服务配置在系统 credential store；Keychain/Secret Service 不可用即明确失败，无明文降级。首次请求从刷新凭证恢复会话，401 刷新一次。网络超时不会宣称写入未发生。

Supabase 邮件模板必须发送 OTP（`{{ .Token }}`），该实现不是浏览器 PKCE；未实现登录链接回调。退出只清理本设备会话，不宣称服务端撤销所有设备会话。没有磁盘团队目录缓存。

## 当前边界

团队 UI 的本机技能详情复用原版详情与诊断组件；本机活动复用原版操作记录的筛选、分页和错误处理。无需云端登录即可读取。安装位置、诊断和 Git 修订仅描述所选本机仓库，不代表团队设备状态，也不改变团队推荐版本。原有高级配置入口仍保留在原版面板中。

本地 install 只导入 registry，activate 是独立步骤；界面应先显示各自预览并分别提交，结果不得合并伪装成跨目标事务。引擎仍执行自身组织策略、信任与冲突检查，native 不提供绕过标志，也不会运行 Skill 脚本。当前 install/activate dry-run 不提供冻结计划 token，apply 会重新执行引擎检查；不宣称具备“预览后内容变化必定失效”的新事务保证。

团队包有真实 team 来源身份，包括服务 origin、team/skill/version ID 与摘要。原生只在本地临时目录保存下载输入，引擎在预览返回前把候选内容存入 registry 的事务目录；apply 不依赖下载临时目录仍存在。更新会预览所有已有投影目标，使用引擎现有事务与恢复日志更新来源、provenance、lock 和投影。来源身份不能被临时路径伪装为 local provider。首次导入不自动激活。

本次只构建 macOS Apple Silicon 的未签名本地 App。真实托管 OTP、Keychain 端到端、干净机器 Git 缺失、各 Agent 会话可见性、签名公证与团队内测尚未验收。认证、凭证和进程调用发布前需要人工审查。测试与产物证据见 [实施验收记录](../docs/plan/loom-desktop-cloud-verification.md)。

POST 云端请求携带 Idempotency-Key，调用者可传入稳定 `idempotencyKey` 以便超时后重试；不传则每次 native 调用生成 UUID，内部 401 重试复用同一值。

本地 debug 构建不访问系统钥匙串：refresh token 仅保存在进程内存，退出 App 后需重新登录。服务地址和 public key 保存在应用配置目录的 `development-cloud-config.json`，不包含登录令牌。release 构建仍使用系统凭证库。
