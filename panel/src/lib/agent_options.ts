/**
 * Canonical list of agent slugs that match the backend `AgentKind` serde
 * wire values (`src/cli.rs` — kebab-case for multi-word variants).
 *
 * This is the single source of truth for form dropdowns and adapter
 * normalisation. Do NOT hard-code agent slugs in individual components.
 */

export interface AgentOption {
  /** Slug sent to the backend; matches `AgentKind` serde wire value. */
  slug: string;
  /** Human label for dropdowns. */
  label: string;
  /** CSS class fragment for colour / avatar lookups (== slug). */
  cssClass: string;
}

export const AGENT_OPTIONS: AgentOption[] = [
  { slug: "claude", label: "Claude Code", cssClass: "claude" },
  { slug: "codex", label: "Codex", cssClass: "codex" },
  { slug: "cursor", label: "Cursor", cssClass: "cursor" },
  { slug: "windsurf", label: "Windsurf", cssClass: "windsurf" },
  { slug: "cline", label: "Cline", cssClass: "cline" },
  { slug: "copilot", label: "Copilot", cssClass: "copilot" },
  { slug: "aider", label: "Aider", cssClass: "aider" },
  { slug: "opencode", label: "OpenCode", cssClass: "opencode" },
  { slug: "gemini-cli", label: "Gemini CLI", cssClass: "gemini-cli" },
  { slug: "goose", label: "Goose", cssClass: "goose" },
];

export const KNOWN_AGENT_SLUGS: readonly string[] = AGENT_OPTIONS.map((o) => o.slug);

/**
 * Normalise a slug coming from the backend for UI purposes. Unknown slugs
 * are returned verbatim (no coercion) so the source of truth — the server
 * payload — stays visible to the user rather than silently relabelled.
 */
export function normalizeAgentSlug(value: string): string {
  const lower = value.toLowerCase().trim();
  return lower;
}
