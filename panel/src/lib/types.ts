/**
 * Known agent slugs — MUST match backend `AgentKind` serde wire values
 * (see `src/cli.rs`, kebab-case for multi-word variants).
 *
 * `AgentSlug` is the string alias used across UI models. Unknown values
 * from the server are preserved as-is (no coercion) — see `toAgentSlug`
 * in `lib/api/adapters.ts`.
 */
export type KnownAgent =
  | "claude"
  | "codex"
  | "cursor"
  | "windsurf"
  | "cline"
  | "copilot"
  | "aider"
  | "opencode"
  | "gemini-cli"
  | "goose";

// Allow unknown slugs so future backend agents render with their real
// name instead of being relabelled. `string & {}` keeps editor hints.
export type AgentSlug = KnownAgent | (string & {});

// Back-compat alias for older call sites that still import AgentKind.
export type AgentKind = AgentSlug;

export type Ownership = "managed" | "observed" | "external";
export type ProjectionMethod = "symlink" | "copy" | "materialize";
export type OpStatus = "ok" | "pending" | "err";

export interface Target {
  id: string;
  agent: AgentSlug;
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
  /**
   * Short form of the latest applied projection revision (first 8 chars
   * of the git hash). Displayed as "latest rev" in UI. Distinct from any
   * notion of "release tag" — the registry may not carry release tags.
   */
  latestRev: string;
  /** Number of rules (binding → target routing entries) that mention this skill. */
  ruleCount: number;
  /** Relative time since the skill's newest projection was last updated. */
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

/**
 * One edge on the projection graph — a skill rendered into a specific
 * target via a specific method. Backed by `RegistryProjection` from the live API.
 */
export interface ProjectionLink {
  skillId: string;
  targetId: string;
  method: ProjectionMethod;
}

export interface TweakState {
  vizMode: VizMode;
  accent: string;
  density: "cozy" | "normal" | "dense";
  compact: boolean;
  hero: "graph" | "grid" | "focus";
  displayFont: "Fraunces" | "Inter" | "JetBrains Mono";
}
