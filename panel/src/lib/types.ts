export type AgentKind =
  | "claude"
  | "codex"
  | "cursor"
  | "windsurf"
  | "cline"
  | "copilot"
  | "aider"
  | "opencode"
  | "gemini"
  | "goose";

export type Ownership = "managed" | "observed" | "external";
export type ProjectionMethod = "symlink" | "copy" | "materialize";
export type OpStatus = "ok" | "pending" | "err";

export interface Target {
  id: string;
  agent: AgentKind;
  profile: string;
  path: string;
  ownership: Ownership;
  skills: number;
  lastSync: string;
}

export interface Skill {
  id: string;
  name: string;
  tag: string;
  version: string;
  captures: number;
  released: string;
  changed: string;
  targets: string[];
}

export interface Op {
  id: string;
  status: OpStatus;
  kind: string;
  skill: string;
  target: string;
  method: ProjectionMethod | "—";
  time: string;
  reason?: string;
}

export interface Binding {
  id: string;
  skill: string;
  target: string;
  matcher: string;
  method: ProjectionMethod;
  policy: "auto" | "manual";
}

export type PanelPageKey =
  | "overview"
  | "skills"
  | "targets"
  | "bindings"
  | "ops"
  | "history"
  | "sync"
  | "settings";

export type VizMode = "loom" | "force" | "tree";

export interface TweakState {
  vizMode: VizMode;
  accent: string;
  density: "cozy" | "normal" | "dense";
  compact: boolean;
  hero: "graph" | "grid" | "focus";
  displayFont: "Fraunces" | "Inter" | "JetBrains Mono";
}
