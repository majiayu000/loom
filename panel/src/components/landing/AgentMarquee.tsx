const AGENTS: { name: string; color: string }[] = [
  { name: "Claude Code", color: "#d97736" },
  { name: "Codex", color: "#6fb78a" },
  { name: "Cursor", color: "#7c8ad9" },
  { name: "Windsurf", color: "#b078c7" },
  { name: "Cline", color: "#9ac078" },
  { name: "Copilot", color: "#c0a678" },
  { name: "Aider", color: "#78c0c0" },
  { name: "OpenCode", color: "#c078a6" },
  { name: "Gemini CLI", color: "#5a8ad2" },
  { name: "Goose", color: "#d2a15a" },
];

export function AgentMarquee() {
  return (
    <section className="marquee">
      <div className="container">
        <div className="marquee-label">One binary, ten agents supported</div>
        <div className="agent-row">
          {AGENTS.map((a) => (
            <div key={a.name} className="agent-pill">
              <span className="dot" style={{ background: a.color }} />
              {a.name}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
