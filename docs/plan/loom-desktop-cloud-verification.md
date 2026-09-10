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
| Frontend | 全量前端 219 项测试通过，团队部分 26 项；TypeScript 检查、lint 与生产构建通过。全库 lint 有 200 条既有警告，团队新增目录单独检查无警告。`frontend-final.log` |
| Browser | 登录页与详情页在桌面和窄屏检查；详情使用明确标注的测试数据，不能当作真实云端账户联调证据 |
| Engine | 团队安装 11 项、命令契约 32 项；inspect 23、provenance 12、workspace init 18、convergence 160、input review 16、org policy 3、provider 2 项回归通过；新增 archive/snapshot/cleanup 检查通过。工作线程 lint、fmt、模块上限通过，35 条既有接近阈值提示。干净 checkout 最终 43 项通过，输出 `engine-clean-final.log`，工作线程分组日志 `worker-engine-*.log` |

原生文件读取审查发现的链接/FIFO 竞态和有损文件名问题已修复，并经只读复核关闭。云端审查发现的条件更新 header、JWT nbf 和密钥刷新问题已修复。一次非 UTF-8 文件测试受到 APFS 文件名限制而失败，改为直接测试实际收集入口使用的 UTF-8 边界，保留拒绝断言。

## 运行与发布边界

桌面构建见 [desktop/README.md](../../desktop/README.md)，服务配置见 [cloud/README.md](../../cloud/README.md)。首批产物为 Apple Silicon 未签名本地调试 App，尚未签名、公证或分发。团队服务需自行配置独立数据库、私有持久卷、TLS 和 Auth。

当前使用邮箱 OTP，Supabase 要配置已验证邮箱 hook 与发送数字验证码的邮件模板。网页令牌仅在内存，刷新后重新登录；桌面刷新凭证由系统凭证库保存。

尚未验收：真实托管 OTP/Keychain 全链路、干净机器安装、Windows、各 Agent 的真实新会话加载、备份恢复演练、限流部署及三团队内测。归档技能仅允许预览恢复已有同来源安装，原生检查本机身份并要求冻结计划代表更新；首次安装被拒绝。手动选择历史版本会建立固定版本计划。源码编译及模拟浏览器数据不替代上述验收。

认证、凭证和进程调用在公开发布前仍需人工审阅。新增 CI 工作流尚未推送，不能宣称远端 CI 已通过。

整合构建发现 `build.rs` 未复制新增 `team.html`，已补齐复制及重新构建触发。当前主目录有 Git 忽略的旧 `skills/loom/SKILL.md`，使严格命令 inventory 检查发现未登记的本机命令面；保留此用户本地文件，最终契约验收使用 `/tmp/loom-team-final-20260908` 干净 checkout，不修改检查规则。

最终桌面产物位于 `desktop/target/debug/bundle/macos/Loom.app`。已用真实 Tauri 窗口验证无需登录进入本机页面、通过包内 CLI 初始化 `/tmp/loom-desktop-smoke-20260908` 并读取本机清单。清单包含已发现但未导入的 Agent 技能，界面提供显式目录导入和预览确认。包内 CLI 与本次源码构建的 sidecar SHA-256 一致，见 `native-smoke.json`；最终构建日志 `desktop-final-bundle.log`。托管认证与团队网络全链路不在此原生冒烟结果内。

线程日志校验脚本只登记旧宿主的 `multi_agent_v1.spawn_agent`；本次在调用进程内登记实际的 `collaboration.spawn_agent` 后使用原 helper 追加，保留其余证据校验、隐私清理和文件锁；未修改安装的 Skill 文件。

## 2026-09-08 原版功能接入团队 UI

本机技能接入原版 `SkillInspectSections`、`SkillDiagnosePanel`，展示来源、安装位置和诊断；新增本机历史与差异入口。独立“本机活动”页面复用 `SkillMAuditHistory` 的分页、筛选和错误处理。新增四个受限只读 native 命令，不提供任意 shell 调用。文件投影成功不再被显示为已验证 Agent 会话可见。

本轮证据位于 `.git/codex/ui-integration/`：前端 33 个测试文件、223 项通过，原生 11 项通过；TypeScript、新增文件 lint、生产构建和 macOS App 打包通过。新增测试覆盖实际安装位置、历史差异参数、切换标签后的过期请求，以及显式读取活动和引擎失败提示。

使用隔离仓库 `/tmp/loom-ui-integration-20260908/registry`，直接创建并提交两次测试技能修订；未修改用户默认仓库。真实 CLI 的 inspect、diagnose、history、diff、ops list 均返回成功，JSON 保存在 `fixture-*.json`。普通无固定摘要的目录安装被原有策略拒绝，测试未绕过策略，不将本轮结果作为安装流程验收。

重新打开本次构建的原生 App 后，验证无需登录进入本机页面、读取测试技能详情、诊断 8 项通过、显示 First revision 到 Second revision 的真实 Git diff，以及本机活动展示 5 条记录。原版高级配置暂留原面板；本轮不是全部旧功能迁移，也不覆盖云端托管登录验收。

产物仍为 `desktop/target/debug/bundle/macos/Loom.app`；包内 CLI 与构建 sidecar 的 SHA-256 均为 `01469675cb6dda686cefb8635bf6b6c4370910d63c8f908688ff66a27f13e05a`。本轮日志为 `native-tests.log`、`frontend-tests.log`、`typecheck.log`、`lint.log`、`build.log`、`engine-build.log` 和 `bundle.log`。

## 2026-09-08 大列表布局调整

实际窗口读取默认仓库后有 209 个条目，原界面的导入表单占满首屏且逐条使用大卡片。已将本机页标题压缩、仓库设置和导入表单默认折叠，改为每页 20 条的紧凑列表，提供全量名称/描述/路径搜索及全部、已导入、未导入、需检查筛选。详情页收起列表和设置，返回时保留筛选状态；异常条目仍明确展示，不改写本地技能文件。

本轮 `.git/codex/ui-polish/` 记录 TypeScript 检查、224 项前端测试、4 个变动文件 lint、生产构建与 App 打包成功。原生窗口复核默认仓库显示 101 个已导入、107 个未导入、1 个需检查；搜索 frontend 并限定已导入返回 3 项，可打开真实技能详情。测试覆盖跨页搜索和状态变化后重置分页。包内 CLI 与 sidecar SHA-256 均为 `b588abfc78ac9416d903b3183f11bcd59071b040b3f4df73a2a87882c597691e`。

## 2026-09-08 Studio 视觉重构

参考 andidea.jp 的深色框架、蓝白画布和胶囊导航，将团队 App 改为顶部导航、工作区选择栏及统一圆角内容面板。欢迎页使用自主绘制的 SVG 编织线条与衬线标题，提供暂停和减少动态效果支持；未使用参考站的图片或视频素材。团队、本机、详情、设置沿用现有业务组件，列表仍保留搜索、状态筛选和每页 20 条。

`.git/codex/studio-ui/` 保存视觉和测试证据：1180×800 团队、本机页面，390×844 欢迎页无横向溢出，浏览器无脚本错误；测试时通过桥接读取本地真实 API 与 CLI 数据，截图不代表浏览器具备真实桌面权限。224 项前端测试通过，TypeScript、变动文件 lint 与生产构建通过。欢迎动效暂停切换已实测。原生 App 重新打包在原路径。

新版原生窗口已打开，真实 Keychain 会话恢复成功，当前账号 `owner@loom.test` 位于“Loom 本地测试团队”。不是模拟浏览器登录状态。

欢迎页现已替换为 imagegen 生成的原创蓝白花鸟丝带插画，素材 `panel/src/assets/loom-blue-garden.png`，保留原生成文件。图片随前端打包，不依赖远程图床；轻微缩放动效可暂停并遵循减少动态效果设置。1180px 和 390px 宽度已检查图片加载、标题可读性与横向溢出；截图为 `generated-art-login.png` 和 `generated-art-mobile.png`。

## 2026-09-09 工作区视觉元素

新增 imagegen 原创纸带、书册、薄片插画 `panel/src/assets/loom-paper-weave.png`，用于团队和更新页头；技能卡片加入轻量装饰印记，团队空状态及本机未读取状态加入折页手册，真实版本历史行增加连接线和节点。全部元素为装饰，不作为业务状态或权限证据。

224 项前端测试、类型检查、3 个变动源文件 lint 和生产构建通过。`.git/codex/studio-ui/elements-*.png` 是明确使用视觉验收数据的 1180px / 390px 截图，窄屏无横向溢出；不把截图中的示例技能写入团队数据库。桌面构建证据为 `elements-engine.log` / `elements-bundle.log`。

## 2026-09-09 三幅页面插画

新增 imagegen 原创蓝白纸艺三幅：团队页 `loom-method-bridge.png` 方法之桥、本机页 `loom-pocket-workshop.png` 口袋工坊、更新页 `loom-paper-steps.png` 向上纸阶。图片随包分发，本机页采用小尺寸装饰，保持列表入口紧凑；窄屏降低装饰对比度，图片不承载业务信息。

类型检查、224 项前端测试、变动文件 lint 和前端构建通过。`.git/codex/studio-ui/new-art-check.mjs` 使用明确的视觉验收样本，检查团队、更新、本机页面 1180px / 390px 展示；团队与本机无横向溢出。截图及 `new-art-*.log` 保存在同目录。

## 2026-09-09 调试版钥匙串弹窗

原代码每次云端调用重新读取钥匙串，当前 App 为 ad-hoc 签名。debug 构建改为仅在内存保存刷新令牌，应用配置目录仅持久化公开连接配置；release 保留 Keychain。未删除旧凭据，未开放钥匙串 ACL。12 项原生测试通过，其中覆盖令牌不落盘及退出登录清除内存令牌；前端类型检查、构建及 App 打包通过。重启后通过原生 UI 读取确认正常显示邮箱登录页，无钥匙串模态弹窗；开发版重启需要重新登录。日志 `keychain-test.log` / `keychain-bundle.log`。

## 2026-09-10 深色目录版预览

蓝白版保留于提交 `682916d`（`codex/desktop-cloud`）及 `~/Applications/Loom-Previews/Loom-Blue.app`。新界面位于 `codex/getdesign-dark-preview`，参考 getdesign.md 首页的黑灰表面、固定侧栏、粉色标题与黄色主按钮，沿用现有业务组件。独立预览包为 `~/Applications/Loom-Previews/Loom-Dark.app`。

类型检查、224 项前端测试、4 个变动文件 lint、构建与 App 打包通过。`.git/codex/studio-ui/dark-check.mjs` 使用视觉验收样本检查登录、团队、本机、更新、设置页面和搜索；390px 页面无横向溢出，无脚本错误。原生窗口已确认加载深色版邮箱登录页。未把视觉样本写入真实团队数据库。

## 2026-09-10 同一 App 内切换两版界面

顶部提供蓝白版/深色版切换按钮，两套样式随包分发，共用业务组件，蓝白版保留插画和顶部导航，深色版保留目录侧栏。选择保存在本机 localStorage，读取或保存失败会在页面明确报错。切换不刷新页面。独立包位于 `~/Applications/Loom-Previews/Loom-Switchable.app`，原有两个预览包保留。

本次重新通过类型检查、5 项 TeamApp 测试、4 个变动 TSX 文件 lint、前端构建和 debug App 打包。Playwright 检查两版 1180px/390px 登录页无横向溢出、无脚本错误，切换保留邮箱输入及本机页位置，刷新恢复所选界面。截图位于 `/tmp/loom-switch-{blue,dark}-{1180,390}.png`。已打开新 App；本次交互验收在浏览器完成。

### 切换入口遮挡修复

原生 App 中蓝白页滚动后，sticky 顶部导航遮挡了原有切换按钮。改为右下角固定的蓝白/深色双按钮，两版外观一致，并以 aria-pressed 表示当前风格；内容底部留出空间。类型检查、5 项 TeamApp 测试、lint、构建、App 打包通过。浏览器检查增加滚动到底后的按钮命中验证；更新后的原生 App 已实际完成深色→蓝白→滚动到底→鼠标坐标点击深色，确认切回成功。
