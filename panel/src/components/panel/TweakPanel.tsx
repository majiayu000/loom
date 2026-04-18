import type { TweakState } from "../../lib/types";

const ACCENTS: { name: string; val: string }[] = [
  { name: "rust", val: "#d97736" },
  { name: "moss", val: "#6fb78a" },
  { name: "indigo", val: "#7c8ad9" },
  { name: "plum", val: "#b078c7" },
  { name: "steel", val: "#c9c0ae" },
];

interface TweakPanelProps {
  state: TweakState;
  onChange: (patch: Partial<TweakState>) => void;
  onDismiss: () => void;
}

export function TweakPanel({ state, onChange, onDismiss }: TweakPanelProps) {
  return (
    <div className="tweaks">
      <div className="t-head">
        <span>Tweaks</span>
        <button onClick={onDismiss} style={{ color: "var(--ink-3)", fontSize: 14 }}>
          ×
        </button>
      </div>
      <div className="t-body">
        <div className="t-group">
          <div className="t-label">Projection viz</div>
          <div className="seg">
            {(["loom", "force", "tree"] as const).map((m) => (
              <button key={m} className={state.vizMode === m ? "on" : ""} onClick={() => onChange({ vizMode: m })}>
                {m}
              </button>
            ))}
          </div>
        </div>
        <div className="t-group">
          <div className="t-label">Accent</div>
          <div className="swatch-row">
            {ACCENTS.map((s) => (
              <div
                key={s.name}
                className={`swatch ${state.accent === s.val ? "on" : ""}`}
                style={{ background: s.val }}
                onClick={() => onChange({ accent: s.val })}
              />
            ))}
          </div>
        </div>
        <div className="t-group">
          <div className="t-label">Density</div>
          <div className="seg">
            {(["cozy", "normal", "dense"] as const).map((d) => (
              <button key={d} className={state.density === d ? "on" : ""} onClick={() => onChange({ density: d })}>
                {d}
              </button>
            ))}
          </div>
        </div>
        <div className="t-group">
          <div className="t-label">Sidebar</div>
          <div className="seg">
            <button className={!state.compact ? "on" : ""} onClick={() => onChange({ compact: false })}>
              expanded
            </button>
            <button className={state.compact ? "on" : ""} onClick={() => onChange({ compact: true })}>
              compact
            </button>
          </div>
        </div>
        <div className="t-group">
          <div className="t-label">Display font</div>
          <div className="seg">
            {(["Fraunces", "Inter", "JetBrains Mono"] as const).map((f) => (
              <button
                key={f}
                className={state.displayFont === f ? "on" : ""}
                onClick={() => onChange({ displayFont: f })}
              >
                {f.split(" ")[0]}
              </button>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
