import type { AgentKind } from "../../lib/types";

interface AgentAvatarProps {
  agent: AgentKind;
  size?: number;
  radius?: number;
}

export function AgentAvatar({ agent, size = 22, radius = 5 }: AgentAvatarProps) {
  return (
    <span
      className={`agent-avatar ${agent}`}
      style={{ width: size, height: size, borderRadius: radius, fontSize: Math.round(size * 0.45) }}
    >
      {agent[0].toUpperCase()}
    </span>
  );
}
