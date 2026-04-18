import type { AgentSlug } from "../../lib/types";

interface AgentAvatarProps {
  agent: AgentSlug;
  size?: number;
  radius?: number;
}

/**
 * Render a small coloured square with the agent's initial.
 *
 * `className` includes the raw slug as a modifier so CSS rules like
 * `.agent-avatar.gemini-cli { ... }` can theme known agents.
 * Unknown slugs fall back to the base `.agent-avatar` styling rather
 * than being coerced into a wrong colour (cf. Codex P2 on PR #7).
 */
export function AgentAvatar({ agent, size = 22, radius = 5 }: AgentAvatarProps) {
  const initial = agent.length > 0 ? agent[0].toUpperCase() : "?";
  return (
    <span
      className={`agent-avatar ${agent}`}
      style={{ width: size, height: size, borderRadius: radius, fontSize: Math.round(size * 0.45) }}
    >
      {initial}
    </span>
  );
}
