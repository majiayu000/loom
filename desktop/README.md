# Loom 桌面壳

独立 Tauri 2 crate，打包 `../panel/dist/team.html` 与版本绑定的 Loom CLI。前端仅在 `window.__TAURI_INTERNALS__` 存在时使用 `@tauri-apps/api/core` 的 `invoke`。浏览器网页没有本地操作权限；远程页面没有 Tauri capability。

## 构建

在仓库根目录运行已有 `make panel-build`（团队入口必须生成 `panel/dist/team.html`），随后：

```sh
./desktop/scripts/prepare-sidecar.sh
cargo check --manifest-path desktop/Cargo.toml --locked
cargo test --manifest-path desktop/Cargo.toml --locked
cd desktop
cargo tauri build
```

开发者需 Rust、Git、Bun、平台 Tauri 构建依赖及 Tauri 2 CLI（`cargo install tauri-cli --version '^2' --locked`）。打包后的用户不需要 Rust/Bun；Git 仍需系统提供。`prepare-sidecar.sh [target-triple]` 编译同 checkout 的 CLI，不下载或调用用户 PATH 中的 loom。原生调用通过 Tauri sidecar 解析打包路径，参数为数组，前端没有通用 shell/文件读取入口。Windows bundle target 需根据发布平台选择；本次默认仅构建 macOS app，未做签名、公证或发布。

## Native bridge

所有名称为 `invoke` 命令名。输入对象字段按下表传入（下划线仅出现于 config 内容字段）；`?` 表示可省略。命令失败 reject 中文 string。CLI 命令直接返回原有 JSON envelope，UI 必须检查其 `ok`，不得把 Promise fulfilled 当安装成功。`root` 不传使用 CLI 默认 registry；显式 root/workspace/source 必须是已有绝对目录。不自动初始化 registry，不扫描任意磁盘。

| 命令 | 输入 | 返回/语义 |
| --- | --- | --- |
| `bootstrap` | 无 | `{git:{available,version? ,message?}}` |
| `choose_directory` / `choose_file` | 无 | 绝对路径或取消时 null |
| `local_skills` | `{root?}` | `loom --json skill list` |
| `inspect_skill` / `deps_skill` | `{root?,skill,agent?,workspace?}` | `skill inspect` / `skill deps` JSON |
| `visibility_skill` | `{root?,skill,agent,workspace?}` | `skill visibility` JSON |
| `preview_install` / `apply_install` | `{root?,source,name}` | `skill install local:<source> --name <name>`，预览追加 `--dry-run` |
| `preview_activate` / `apply_activate` | `{root?,skill,agent,workspace?}` | `skill activate --agent`，有 workspace 为 project，否则 user；预览追加 `--dry-run` |
| `get_cloud_config` | 无 | `{cloud_api_url,auth_url,auth_public_key}`，未设置时均空字符串 |
| `save_cloud_config` | `{config:{cloud_api_url,auth_url,auth_public_key}}` | 保存系统凭证库并清除之前的会话 |
| `request_otp` | `{email}` | Supabase `POST /auth/v1/otp`；成功 void |
| `verify_otp` | `{email,token}` | Supabase `POST /auth/v1/verify` type=email；再向 auth user 端点验证并返回 `{id,email}` |
| `current_user` | 无 | 通过刷新凭证及 auth user 端点返回 `{id,email}`；未登录 null |
| `cloud_request` | `{method,path,body?,idempotencyKey?}` | `/v1/` 云端 API 成功 JSON；原生添加 bearer，不返回凭证 |
| `logout` | 无 | 删除持久刷新凭证与内存访问令牌，保留服务配置 |

`cloud_api_url` 为 API origin；`auth_url` 为 Supabase 项目 origin，均不带路径。远程必须 HTTPS，本机开发可用 HTTP localhost/127.0.0.1。请求禁用重定向，路径不能逃逸 `/v1/`。支持 GET/POST/PUT/PATCH/DELETE；JSON body 可选。访问令牌只在 Rust 内存，刷新凭证与服务配置在系统 credential store；Keychain/Secret Service 不可用即明确失败，无明文降级。首次请求从刷新凭证恢复会话，401 刷新一次。网络超时不会宣称写入未发生。

Supabase 邮件模板必须发送 OTP（`{{ .Token }}`），该实现不是浏览器 PKCE；未实现登录链接回调。退出只清理本设备会话，不宣称服务端撤销所有设备会话。没有磁盘团队目录缓存。

## 当前边界

本地 install 只导入 registry，activate 是独立步骤；界面应先显示各自预览并分别提交，结果不得合并伪装成跨目标事务。引擎仍执行自身组织策略、信任与冲突检查，native 不提供绕过标志，也不会运行 Skill 脚本。当前 install/activate dry-run 不提供冻结计划 token，apply 会重新执行引擎检查；不宣称具备“预览后内容变化必定失效”的新事务保证。

云端 provider 安装、包读取上传/下载、更新/回滚和云端来源身份接入尚未由此壳实现。不得将下载到临时目录的云端内容当 local source 冒充云端安装。后续应新增受限具名命令并使用引擎真实 provider 契约，凭证通过 native IPC/stdin 传递，不加入命令行。

测试只验证 native 输入边界，不操作用户 Skill 目录。需要另行完成真实 keychain/OTP 服务、桌面 UI、干净机器 Git 缺失、Agent 可见性、安装中断恢复、签名包验收。涉及认证、凭证和进程调用，发布前需要人工审查。

## 本次原生验证

2026-09-08 在 macOS Apple Silicon 上完成 5 个 native 边界单元测试。隔离 checkout 当时没有团队前端 dist，测试使用 `TAURI_CONFIG='{"build":{"frontendDist":".artifacts/ui"}}' cargo test --manifest-path desktop/Cargo.toml --locked`，其中 `.artifacts/ui/team.html` 是仅供编译的临时 fixture。sidecar 编译资源使用本机既有 debug loom，未在此测试启动它；正式包必须运行上述 prepare 脚本。未将这些结果视为完整 UI、正式发布包或身份服务验收。图标由已有 `panel/public/favicon.svg` 渲染，没有新增品牌设计。日志在忽略目录 `.artifacts/`。

POST 云端请求携带 Idempotency-Key，调用者可传入稳定 `idempotencyKey` 以便超时后重试；不传则每次 native 调用生成 UUID，内部 401 重试复用同一值。
