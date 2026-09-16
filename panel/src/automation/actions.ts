export interface Field {
  key: string;
  label: string;
  hint?: string;
  options?: string[];
  optional?: boolean;
  multiline?: boolean;
}
export interface Action {
  id: string;
  group: string;
  label: string;
  description: string;
  effect: string;
  preview?: boolean;
  review?: boolean;
  fields: Field[];
}
const name = { key: "name", label: "技能或集合名称" };
const workspace = { key: "workspace", label: "工作目录", hint: "本机绝对路径" };
const plan = { key: "plan", label: "已审阅计划", hint: "计划 ID 或计划文件路径" };
const output = { key: "output", label: "输出路径", hint: "填写新的文件或目录路径" };
const key = { key: "idempotency_key", label: "执行标识", hint: "同一次操作重试时保留此值" };
const approvals = { key: "approvals", label: "计划要求的审批名称", hint: "逐行填写计划返回的 required_approvals", multiline: true, optional: true };
const artifact = { key: "artifact", label: "配置包或产物路径" };
export const actions: Action[] = [
  { id: "author_rewrite", group: "技能编写", label: "改写技能", description: "审阅发送给 Codex 的内容，再生成补丁。", effect: "使用本机 Codex 模型；生成待审阅补丁，原技能保持不变。", preview: true, review: true, fields: [name, { key: "instruction", label: "改写要求", multiline: true }] },
  { id: "author_draft", group: "技能编写", label: "从会话起草", description: "从你指定的会话材料起草一个新技能。", effect: "将指定材料交给本机 Codex；生成补丁供后续应用。", preview: true, review: true, fields: [name, { key: "session", label: "已审阅的会话文件路径" }] },
  { id: "author_apply", group: "技能编写", label: "应用补丁", description: "将已审阅补丁应用到技能；已有内容和路径会重新校验。", effect: "写入技能文件。请先审阅生成的补丁和校验结果。", review: true, fields: [{ key: "patch", label: "补丁 ID" }, key] },
  { id: "skillset_eval", group: "评测", label: "整包对照评测", description: "比较整个技能集合与无技能或单个技能的表现。", effect: "Codex 模式会调用本机模型；mock 仅用于流程验证。", preview: true, review: true, fields: [name, { key: "runner", label: "运行方式", options: ["codex-cli", "mock"] }, { key: "baseline", label: "对照组", options: ["no-skill", "single-skills"] }] },
  { id: "workflow_create", group: "工作流", label: "从集合创建", description: "按集合成员顺序创建串行工作流快照。", effect: "保存工作流定义，不执行节点。", preview: true, review: true, fields: [{ key: "name", label: "工作流名称" }, { key: "skillset", label: "技能集合名称" }] },
  { id: "workflow_plan", group: "工作流", label: "生成执行计划", description: "检查工作流、源文件与激活状态，列出需要的审批。", effect: "保存计划，不调用模型。", fields: [{ key: "name", label: "工作流名称" }, workspace] },
  { id: "workflow_apply", group: "工作流", label: "执行已审阅计划", description: "先重新校验计划，再通过 Codex 执行节点。", effect: "会调用模型；允许写入的节点会修改所选工作目录。", preview: true, review: true, fields: [plan, { key: "inputs", label: "输入 JSON 文件路径", optional: true }, approvals, key] },
  { id: "instruction_scan", group: "指令提取", label: "扫描项目指令", description: "查找项目中的原生指令文件及其作用范围。", effect: "只读检查。", fields: [workspace] },
  { id: "instruction_migrate", group: "指令提取", label: "生成提取补丁", description: "将指定指令提取为新技能或已有技能的参考文件。", effect: "保存待审阅补丁；源指令文件保持不变。", preview: true, review: true, fields: [{ key: "instruction_id", label: "扫描返回的指令 ID" }, workspace, name, { key: "target", label: "提取到", options: ["skill", "reference"] }] },
  { id: "package_plan", group: "打包", label: "生成打包计划", description: "选择技能或集合，审阅要分发的文件和元数据。", effect: "保存计划文件，尚不创建分发包。", fields: [{ key: "source", label: "来源", hint: "skill:名称 或 skillset:名称" }, { key: "format", label: "分发格式", options: ["codex-plugin", "claude-plugin", "npm", "github-release", "agent-skills-archive"] }, output] },
  { id: "package_build", group: "打包", label: "构建分发包", description: "从已审阅的计划构建可校验产物。", effect: "写入本机分发包；不会发布到外部服务。", review: true, fields: [plan, output, key] },
  { id: "package_verify", group: "打包", label: "校验分发包", description: "检查文件摘要、格式元数据和内容。", effect: "只读检查。", fields: [artifact] },
  { id: "provision_plan", group: "环境配置", label: "生成配置计划", description: "为工作目录准备可移植的环境配置。", effect: "保存计划，不写入目标工作目录。", fields: [{ key: "target", label: "目标环境", options: ["remote", "devcontainer", "codespaces"] }, workspace] },
  { id: "provision_export", group: "环境配置", label: "导出配置包", description: "将审阅后的配置导出，供另一台机器使用。", effect: "生成本机产物；远端脚本由你显式执行。", review: true, fields: [plan, { key: "format", label: "产物格式", options: ["tar", "shell", "devcontainer"] }, output] },
  { id: "provision_import", group: "环境配置", label: "导入配置包", description: "校验配置包，并解包到新的输出目录。", effect: "写入新的目录，不执行包内脚本。", preview: true, review: true, fields: [artifact, output] },
  { id: "provision_apply", group: "环境配置", label: "应用配置计划", description: "重新检查已审阅计划后写入本机配置。", effect: "写入计划中的本机配置文件，不连接远端。", review: true, fields: [plan, approvals, key] },
  { id: "catalog_search", group: "外部目录", label: "搜索技能", description: "从已配置的 Provider 查找技能。", effect: "允许联网搜索；GitHub 使用本机 gh 登录。", fields: [{ key: "provider", label: "已配置 Provider ID" }, { key: "query", label: "搜索关键词" }] },
  { id: "catalog_preview", group: "外部目录", label: "检查远程技能", description: "拉取临时副本，检查来源、lint 和安全结果。", effect: "允许联网拉取，不安装技能、不执行技能脚本。", fields: [{ key: "locator", label: "Provider 来源地址", hint: "github:owner/repo//skills/name@ref" }] },
];

export function defaults(action: Action): Record<string, string> {
  return Object.fromEntries(action.fields.map(field => [field.key, field.key === "idempotency_key" ? crypto.randomUUID() : field.options?.[0] ?? ""]));
}

export function requestFor(action: Action, values: Record<string, string>, preview: boolean): Record<string, unknown> {
  const request: Record<string, unknown> = { action: action.id };
  for (const field of action.fields) {
    const value = values[field.key] ?? "";
    request[field.key] = field.key === "approvals" ? value.split("\n").map(v => v.trim()).filter(Boolean) : value;
  }
  if (action.preview) request.dry_run = preview;
  return request;
}
