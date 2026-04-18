interface Concept {
  num: string;
  title: string;
  body: string;
  code: string;
}

const CONCEPTS: Concept[] = [
  {
    num: "01 / CONCEPT",
    title: "Target",
    body: "An agent skills directory Loom knows about.",
    code: "~/.claude/skills",
  },
  {
    num: "02 / CONCEPT",
    title: "Skill",
    body: "A versioned unit in the registry.",
    code: "refactor-patterns@v0.4",
  },
  {
    num: "03 / CONCEPT",
    title: "Binding",
    body: "Rule mapping a skill to a target.",
    code: "agent=claude, profile=work",
  },
  {
    num: "04 / CONCEPT",
    title: "Projection",
    body: "Realize a skill into a target.",
    code: "--method symlink",
  },
];

const TARGET_ROWS: { label: string; path: string; dotColor: string }[] = [
  { label: "claude / home", path: "~/.claude/skills", dotColor: "#d97736" },
  { label: "codex / home", path: "~/.codex/skills", dotColor: "#6fb78a" },
  { label: "cursor / repo", path: "/repo/.cursor/skills", dotColor: "#7c8ad9" },
  { label: "windsurf / home", path: "~/.windsurf/skills", dotColor: "#b078c7" },
];

export function HowItWorks() {
  return (
    <section className="section" id="how" style={{ paddingTop: 40 }}>
      <div className="container">
        <div className="section-head">
          <div className="section-eyebrow">How it works</div>
          <h2 className="section-title">Four concepts. One control plane.</h2>
          <p className="section-sub">
            Registry holds the truth. Bindings describe the rules. Projection writes reality into each agent's
            directory.
          </p>
        </div>

        <div className="how-wrap">
          <div className="how-diagram">
            <svg viewBox="0 0 880 320">
              <defs>
                <marker id="how-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                  <path d="M0 0 L10 5 L0 10 z" fill="#d97736" />
                </marker>
              </defs>

              {/* Registry box */}
              <g>
                <rect x="30" y="40" width="260" height="150" rx="10" fill="#1c1915" stroke="#2a2620" />
                <text x="50" y="66" fill="#5a5346" fontFamily="JetBrains Mono" fontSize="10" letterSpacing="1.5">
                  REGISTRY
                </text>
                <text x="50" y="92" fill="#f4ede0" fontFamily="Fraunces" fontSize="19">
                  your Git repo
                </text>
                <text x="50" y="120" fill="#c9c0ae" fontFamily="JetBrains Mono" fontSize="12">
                  skills/*
                </text>
                <text x="50" y="140" fill="#c9c0ae" fontFamily="JetBrains Mono" fontSize="12">
                  versions/*
                </text>
                <text x="50" y="160" fill="#c9c0ae" fontFamily="JetBrains Mono" fontSize="12">
                  bindings.json
                </text>
                <text x="50" y="180" fill="#c9c0ae" fontFamily="JetBrains Mono" fontSize="12">
                  ops/*
                </text>
              </g>

              {/* Loom core */}
              <g>
                <rect x="350" y="90" width="180" height="60" rx="8" fill="#2a1810" stroke="#d97736" />
                <text x="440" y="116" textAnchor="middle" fill="#d97736" fontFamily="Fraunces" fontSize="20" fontWeight="500">
                  loom
                </text>
                <text x="440" y="136" textAnchor="middle" fill="#c9c0ae" fontFamily="JetBrains Mono" fontSize="11">
                  CLI · Panel
                </text>
              </g>

              {/* Target dirs */}
              <g>
                <rect x="590" y="20" width="260" height="280" rx="10" fill="#1c1915" stroke="#2a2620" />
                <text x="610" y="46" fill="#5a5346" fontFamily="JetBrains Mono" fontSize="10" letterSpacing="1.5">
                  TARGET DIRS
                </text>
                {TARGET_ROWS.map((t, i) => {
                  const y = 60 + i * 58;
                  return (
                    <g key={t.label}>
                      <rect x="610" y={y} width="220" height="50" rx="6" fill="#0e0d0b" stroke="#2a2620" />
                      <circle cx="628" cy={y + 25} r="4" fill={t.dotColor} />
                      <text x="642" y={y + 22} fill="#f4ede0" fontFamily="Inter" fontSize="13" fontWeight="500">
                        {t.label}
                      </text>
                      <text x="642" y={y + 39} fill="#8a8271" fontFamily="JetBrains Mono" fontSize="10">
                        {t.path}
                      </text>
                    </g>
                  );
                })}
              </g>

              {/* Arrows */}
              <path d="M 290 120 C 320 120, 320 120, 349 120" stroke="#d97736" strokeWidth="1.5" fill="none" markerEnd="url(#how-arrow)" />
              <text x="300" y="108" fill="#8a8271" fontFamily="JetBrains Mono" fontSize="10">
                capture
              </text>
              <text x="300" y="138" fill="#8a8271" fontFamily="JetBrains Mono" fontSize="10">
                release
              </text>
              <path d="M 530 120 C 560 120, 560 85, 589 85" stroke="#d97736" strokeWidth="1.5" fill="none" markerEnd="url(#how-arrow)" />
              <path d="M 530 120 C 560 120, 560 143, 589 143" stroke="#d97736" strokeWidth="1.5" fill="none" markerEnd="url(#how-arrow)" />
              <path d="M 530 120 C 560 120, 560 201, 589 201" stroke="#d97736" strokeWidth="1.5" fill="none" markerEnd="url(#how-arrow)" />
              <path d="M 530 120 C 560 120, 560 259, 589 259" stroke="#d97736" strokeWidth="1.5" fill="none" markerEnd="url(#how-arrow)" />
              <text x="540" y="78" fill="#8a8271" fontFamily="JetBrains Mono" fontSize="10">
                project
              </text>
            </svg>
          </div>

          <div className="how-concepts">
            {CONCEPTS.map((c) => (
              <div key={c.title} className="how-concept">
                <div className="num">{c.num}</div>
                <h4>{c.title}</h4>
                <p>{c.body}</p>
                <code>{c.code}</code>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
