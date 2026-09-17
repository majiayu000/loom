# 本机工作台与 Provider 验收

日期：2026-09-17。验收使用专用临时 registry、工作目录和输出目录；没有修改用户凭证或原有技能。

## 本轮实现

- 默认 Panel 的 Workbench 接入技能起草、改写、补丁应用、整包评测、工作流、指令提取、打包和环境配置；Market 默认打开 Provider 搜索。桌面“技能工作台”使用同一组件，无需云端登录。
- 18 个固定类型的操作请求映射已有 CLI；HTTP 入口继续检查本机来源，桌面调用打包 sidecar。请求不接收任意命令或 shell，参数不能改写命令或全局 registry。
- 模型调用及支持 dry-run 的写入先展示预览，再显式确认。应用已有计划或补丁也需确认；改变参数使确认失效。引擎仍重新验证计划、权限和来源。网络中断后应先检查操作记录；有执行标识的重试保留原值。
- GitHub 搜索通过本机已登录的 gh 查询已配置 Provider，显示总数、截断和来源信息。远程预览在临时 checkout 检查 lint、安全和 provenance，返回固定提交的 locator；不安装、不执行技能脚本，结束清理临时目录。
- CLI contract 升至 2.1.0。修正此前 CI 的旧契约范围测试数据，并补齐 team-install 的公开命令示例。指令迁移设计文档同步为保留源指令、生成补丁、显式应用的实际行为。

## 已验证

| 场景 | 结果 |
| --- | --- |
| 真实 GitHub 搜索 | 已配置 Provider 查询 openai/skills，返回 gh-fix-ci 技能 |
| 真实远程预览 | 固定提交 49f948faa9258a0c61caceaf225e179651397431，lint 有效，scripts_executed=false |
| 浏览器改写预览 | 实际 HTTP 返回 provider=codex-cli、artifact_written=false；未触发模型调用 |
| 浏览器打包 | 表单生成计划、确认构建 Codex 插件包、校验摘要与源内容，valid=true |
| 浏览器指令与 Remote 配置 | 生成提取补丁后源 AGENTS.md 不变；Remote tar 导出、预览与新目录导入通过，scripts_executed=false |
| 浏览器界面 | 1180×900 和 390×844 检查，无新增页面异常或横向溢出 |
| 前端 | 35 个测试文件、235 项测试通过；语句覆盖率 77.68%，分支 72.07%，函数 74.12%，行 81.75%；类型检查与生产构建通过 |
| 后端与桥接 | 5 项工作台测试、14 项桌面测试通过，覆盖全部操作映射、参数隔离、来源授权和真实错误传播 |
| Provider 回归 | Provider CLI 2 项、远程 Provider 2 项、provenance 12 项通过 |
| 发布契约回归 | release_contract 18 项通过、1 项需打包环境而忽略；shipped_registry_skill 3 项通过；公开命令文档检查通过 |
| 严格命令契约 | 32 项检查通过；在不含本机忽略文件的隔离 checkout 中执行，比较基线为 b979f06 |
| 静态检查 | 全目标 Clippy、Rust 格式和模块上限检查通过；前端 lint 无错误，SkillMPanel 原有 5 条警告仍在 |
| 本机发布包准备 | 当前 debug 二进制、第一方 Skill 和契约清单原子组包并校验成功，contract=2.1.0；这不是公开 release 产物 |

原始 JSON、浏览器截图和组包摘要保存在本机 `output/workbench-followup-20260917/`。测试日志位于 `/tmp/loom-workbench-*.log`。测试产物不纳入发布包或版本控制。

## CI 后续修复

首次推送后的 Windows 检查和 Team App 流水线通过（含 macOS App 构建）。Linux 的完整 trace 检查暴露过期工作流计划的重建提示遗漏 agent/workspace，现已修复并由公开 CLI 解析器校验；12 项工作流回归通过。macOS 的单项 trash 健康断言曾失败，本机单次及 24 次并发重复均未复现；保留原断言并补充完整 doctor 诊断，继续由 CI 验证。

## 资源体积优化

完整 CI 的 Windows、macOS 与 Team App 已通过，Linux 进一步暴露旧的发布体积门槛。四张团队插图保留原始 PNG，并以 `cwebp -q 82 -m 6` 生成 WebP 供界面引用，资源从 7,431,214 字节降至 445,478 字节；HTTP 增加对应 MIME 类型。移除源码已无引用的原型页面样式后，Panel gzip 包为 113,528 字节，低于原有 114,688 字节门槛。

优化后 235 项前端测试及覆盖率检查、类型检查、Clippy 和 WebP MIME 回归通过；工作台、概览、技能列表、设置和蓝白团队页完成浏览器复验，图片加载正常且无页面异常。Linux 二进制大小仍由原有 6,448,128 字节门槛检查，未放宽阈值。

## 保留回溯的 Linux 体积验收

用户选择保留崩溃回溯后，实测发现原有 `--no-eh-frame-hdr` 参数已经使 Linux panic 仅打印回溯标题、没有栈帧。本轮移除该参数，并让性能检查通过 `/dev/full` 触发一次真实 stdout panic：缺失回溯的旧产物会被拒绝。展开表、查找表、PIE/ASLR 和 RELRO 均保留。

代码调整取消执行和审计时对整棵 CLI 参数树的克隆；对排序键使用标准库缓存排序，保持同键记录的原始顺序；部分临时 BTreeMap/BTreeSet 索引改为逐项插入，减少为不同类型生成的批量排序代码。将这些索引改回 `collect()` 前应重新测量 release 体积。

Linux x86_64 发布配置启用固定帧指针，简化栈访问和展开记录；LLD 合并相同函数，并用 RELR 压缩相对重定位表。公开 0.1.8 Linux 产物的符号版本检查已经要求 glibc 2.39；RELR 所需的 glibc 2.36 不会提高这一既有要求。README 和发布文档现已说明实际运行要求。

使用与 CI 相同的 Rust 1.98.1、正式发布参数构建 x86_64 Linux 二进制，链接 Debian 12 的 glibc 2.36 和原生 GNU 启动对象，在隔离的 Debian 12 容器中执行仓库检查：

| 检查 | 结果 |
| --- | --- |
| 二进制大小 | 6,413,656 字节，低于 6,448,128 字节门槛 34,472 字节 |
| 真实崩溃回溯 | 18 个栈帧；移除查找表的对照产物被新检查拒绝 |
| 冷启动 p95 | `--version` 57.6 ms，`--help` 38.0 ms，均低于 300 ms |
| 查询 p95 | workspace status 300.3 ms、skill inspect 263.7 ms，均低于 1,000 ms |
| 前端 gzip | 113,528 字节，低于未变更的 114,688 字节上限 |
| 发布二进制 E2E | `scripts/e2e-agent-flow.sh` A–F 全部通过，覆盖投影、Agent 可见性与收敛重试 |
| 源码回归 | 444 项单元测试；inventory 10、recommend 15、stats 23、workflow 12 项集成测试通过 |
| 静态检查 | 全目标 Clippy、格式、模块上限与 shell 语法检查通过 |

本地交叉构建使用独立工具链与临时 Linux sysroot，没有更改全局 Rust 默认版本。上述数据为本机容器验收；推送 `c1fdbed` 后的主 CI 已通过 Ubuntu/macOS/Windows 与 Team App（含原生 Linux 发布构建和体积检查）。性能日志和构建证据保存在本机 `output/release-size-20260917/`，不随发布包分发。

## 验收边界

本轮没有重复真实模型调用；已有真实 Codex 编写、工作流交接和评测验收见 [Codex CLI 验收](codex-cli-verification.md)。本机包使用 debug 二进制，公开版本仍为 0.1.8；CLI contract 版本与产品 release 版本独立。

Remote 仍按已确定的配置包导出、导入、目标机器显式执行方式实现。跨机器脚本执行、全部 Agent 插件加载、托管 OTP/Keychain 端到端、签名公证、公开发布与团队内测尚未完成。认证、凭证和进程调用公开发布前仍需人工审阅。
