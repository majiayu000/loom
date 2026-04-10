# Loom 完整文档（中文，v3-only）

更新时间：2026-04-10

## 1. 定位

Loom 是 **Skill Registry 管理工具**，不是 skill 内容仓库本身。

- Loom 仓库：工具源码（你现在看的这个仓库）
- Registry 仓库：真实的 `skills/` + `state/v3/*`（独立 Git 仓库）

核心原则：

1. `skills/<skill>` 是事实源（canonical source）
2. `target` 和 `binding` 显式建模，拒绝隐式目录推断
3. 不提供 v1/v2 兼容命令面

## 2. 已移除命令

以下命令已移除，不再支持：

- `workspace init`
- `skill import`
- `skill link`
- `skill use`
- `migrate v2-to-v3`

## 3. 关键对象

- `Target`：一个可投影目录（路径、ownership、capabilities）
- `Workspace Binding`：工作区匹配规则 + 默认 target
- `Projection Instance`：某个 skill 在某个 binding/target 的一次投影实例

## 4. Root 安全保护

写操作会拒绝 Loom 工具仓库本身作为 `--root`：

- 目的：避免误把工具仓库当业务 registry 改写
- 方式：直接返回 `ARG_INVALID`

请始终使用独立 registry 根目录：

```bash
loom --json --root /path/to/your-registry ...
```

## 5. 快速开始（独立 Registry）

### 5.1 创建并初始化 registry 仓库

```bash
mkdir -p ~/loom-registry
cd ~/loom-registry
git init
```

### 5.2 添加 skill（事实源）

```bash
loom --json --root ~/loom-registry skill add /path/to/my-skill --name my-skill
```

### 5.3 注册 target（支持多目录）

```bash
loom --json --root ~/loom-registry target add --agent claude --path "$HOME/.claude/skills" --ownership managed
loom --json --root ~/loom-registry target add --agent claude --path "$HOME/.claude-work/skills" --ownership managed
loom --json --root ~/loom-registry target add --agent codex --path "$HOME/.codex/skills" --ownership managed
```

### 5.4 绑定 workspace 到默认 target

```bash
loom --json --root ~/loom-registry workspace binding add \
  --agent claude \
  --profile project-a \
  --matcher-kind path-prefix \
  --matcher-value /Users/you/work/project-a \
  --target target_claude_claude_skills
```

### 5.5 投影、编辑、回写

```bash
# 投影到默认 target
loom --json --root ~/loom-registry skill project my-skill --binding bind_claude_project_a --method symlink

# 或显式选择另一个 target
loom --json --root ~/loom-registry skill project my-skill --binding bind_claude_project_a --target target_claude_claude_work_skills --method copy

# 编辑 live 文件后，按实例回写
loom --json --root ~/loom-registry skill capture my-skill --instance <instance-id> --message "capture live edits"
```

## 6. 当前命令面

### 6.1 workspace

- `workspace status`
- `workspace doctor`
- `workspace binding add|list|show|remove`
- `workspace remote set|status`

### 6.2 target

- `target add|list|show|remove`

### 6.3 skill

- `skill add`
- `skill project`
- `skill capture`
- `skill save`
- `skill snapshot`
- `skill release`
- `skill rollback`
- `skill diff`

### 6.4 sync

- `sync status|push|pull|replay`

### 6.5 ops

- `ops list|retry|purge`
- `ops history diagnose|repair --strategy <local|remote>`

### 6.6 panel

- `panel --port <port>`

## 7. 常见错误码

- `TARGET_NOT_FOUND`：绑定或投影引用了不存在的 target
- `BINDING_NOT_FOUND`：找不到 binding
- `SKILL_NOT_FOUND`：找不到 skill
- `DEPENDENCY_CONFLICT`：删除 target 时仍有 binding/rule/projection 依赖
- `ARG_INVALID`：参数不合法或触发 root 保护

## 8. status 字段说明（重点）

`workspace status` 里与目录相关的两个字段语义不同：

- `agent_dir_defaults`：环境默认目录（诊断用途）
- `registered_targets`：registry 内已注册 target（真实执行对象）

请以 `registered_targets` 和 `v3.targets` 为准。

## 9. 一键 Agent E2E

仓库内置真实四场景 E2E：

- A：`.claude/skills` + `symlink`
- B：`.claude-work/skills` + `copy`
- C：多目录 target 显式选择
- D：`.codex/skills` + 失败反馈验证

运行：

```bash
./scripts/e2e-agent-flow.sh
```

指定输出目录：

```bash
./scripts/e2e-agent-flow.sh /tmp/my-loom-e2e
```

## 10. 本地与 CI 统一入口

```bash
make fmt-check
make lint
make test
make panel-build
make e2e
make ci
```

`.github/workflows/ci.yml` 与 `Makefile` 保持同一验证路径。

## 11. 工具仓库与 Registry 仓库建议

建议结构：

```text
~/code/infra/loom            # 工具源码仓库
~/code/registry/loom-skills  # 实际业务 registry 仓库
```

执行 Loom 时始终指向 registry：

```bash
loom --json --root ~/code/registry/loom-skills ...
```

