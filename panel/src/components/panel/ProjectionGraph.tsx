import { useMemo } from "react";
import type { Ownership, ProjectionMethod, Skill, Target, VizMode } from "../../lib/types";

interface SkillNode {
  id: string;
  x: number;
  y: number;
  y2?: number;
  label: string;
}

interface TargetNode {
  id: string;
  x: number;
  y: number;
  x2?: number;
  label: string;
  agent: string;
  ownership: Ownership;
}

interface LayoutResult {
  mode: VizMode;
  width: number;
  height: number;
  skills: SkillNode[];
  targets: TargetNode[];
}

const WIDTH = 860;
const HEIGHT = 440;

function useLayout(mode: VizMode, skills: Skill[], targets: Target[]): LayoutResult {
  return useMemo(() => {
    if (mode === "loom") {
      const leftPad = 160;
      const rightPad = 160;
      const topPad = 50;
      const botPad = 60;
      const colStep = (WIDTH - leftPad - rightPad) / Math.max(skills.length - 1, 1);
      const rowStep = (HEIGHT - topPad - botPad) / Math.max(targets.length - 1, 1);
      return {
        mode,
        width: WIDTH,
        height: HEIGHT,
        skills: skills.map((s, i) => ({
          id: s.id,
          x: leftPad + i * colStep,
          y: topPad,
          y2: HEIGHT - botPad,
          label: s.name,
        })),
        targets: targets.map((t, i) => ({
          id: t.id,
          y: topPad + i * rowStep,
          x: leftPad,
          x2: WIDTH - rightPad,
          label: `${t.agent}/${t.profile}`,
          agent: t.agent,
          ownership: t.ownership,
        })),
      };
    }

    if (mode === "force") {
      const leftX = 200;
      const rightX = WIDTH - 200;
      return {
        mode,
        width: WIDTH,
        height: HEIGHT,
        skills: skills.map((s, i) => ({
          id: s.id,
          x: leftX,
          y: 40 + i * ((HEIGHT - 80) / Math.max(skills.length - 1, 1)),
          label: s.name,
        })),
        targets: targets.map((t, i) => ({
          id: t.id,
          x: rightX,
          y: 40 + i * ((HEIGHT - 80) / Math.max(targets.length - 1, 1)),
          label: `${t.agent}/${t.profile}`,
          agent: t.agent,
          ownership: t.ownership,
        })),
      };
    }

    return {
      mode,
      width: WIDTH,
      height: HEIGHT,
      skills: skills.map((s, i) => ({
        id: s.id,
        x: 60 + i * ((WIDTH - 120) / Math.max(skills.length - 1, 1)),
        y: 60,
        label: s.name,
      })),
      targets: targets.map((t, i) => ({
        id: t.id,
        x: 60 + i * ((WIDTH - 120) / Math.max(targets.length - 1, 1)),
        y: HEIGHT - 80,
        label: `${t.agent}/${t.profile}`,
        agent: t.agent,
        ownership: t.ownership,
      })),
    };
  }, [mode, skills, targets]);
}

function ownershipColor(o: Ownership): string {
  if (o === "managed") return "#d97736";
  if (o === "observed") return "#6fb78a";
  return "#8a8271";
}

function methodColor(m: ProjectionMethod): string {
  if (m === "symlink") return "#6fb78a";
  if (m === "copy") return "#e6b450";
  return "#c79ee0";
}

interface ProjectionRecord {
  skill: string;
  target: string;
  method: ProjectionMethod;
  ownership: Ownership;
}

function buildProjections(skills: Skill[], targets: Target[]): ProjectionRecord[] {
  const methods: ProjectionMethod[] = ["symlink", "copy", "materialize"];
  const out: ProjectionRecord[] = [];
  skills.forEach((s) => {
    s.targets.forEach((tid) => {
      const t = targets.find((x) => x.id === tid);
      if (!t) return;
      const method = methods[(s.id.length + t.id.length) % 3];
      out.push({ skill: s.id, target: tid, method, ownership: t.ownership });
    });
  });
  return out;
}

interface ProjectionGraphProps {
  mode?: VizMode;
  selectedSkill: string | null;
  selectedTarget: string | null;
  onSelectSkill: (id: string) => void;
  onSelectTarget: (id: string) => void;
  skills: Skill[];
  targets: Target[];
}

export function ProjectionGraph({
  mode = "loom",
  selectedSkill,
  selectedTarget,
  onSelectSkill,
  onSelectTarget,
  skills,
  targets,
}: ProjectionGraphProps) {
  const layout = useLayout(mode, skills, targets);
  const projections = useMemo(() => buildProjections(skills, targets), [skills, targets]);

  const isHi = (sid: string | null, tid: string | null): boolean => {
    if (!selectedSkill && !selectedTarget) return true;
    if (selectedSkill && sid === selectedSkill) return true;
    if (selectedTarget && tid === selectedTarget) return true;
    return false;
  };

  return (
    <svg
      viewBox={`0 0 ${layout.width} ${layout.height}`}
      style={{ width: "100%", height: "100%", display: "block" }}
    >
      <defs>
        <linearGradient id="warp-grad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#d97736" stopOpacity="0.1" />
          <stop offset="50%" stopColor="#d97736" stopOpacity="0.55" />
          <stop offset="100%" stopColor="#d97736" stopOpacity="0.1" />
        </linearGradient>
        <filter id="node-glow">
          <feGaussianBlur stdDeviation="2" result="b" />
          <feMerge>
            <feMergeNode in="b" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {layout.mode === "loom" && (
        <LoomMode
          layout={layout}
          projections={projections}
          selectedSkill={selectedSkill}
          selectedTarget={selectedTarget}
          isHi={isHi}
          onSelectSkill={onSelectSkill}
          onSelectTarget={onSelectTarget}
        />
      )}

      {layout.mode === "force" && (
        <ForceMode
          layout={layout}
          projections={projections}
          selectedSkill={selectedSkill}
          selectedTarget={selectedTarget}
          isHi={isHi}
          onSelectSkill={onSelectSkill}
          onSelectTarget={onSelectTarget}
        />
      )}

      {layout.mode === "tree" && (
        <TreeMode
          layout={layout}
          projections={projections}
          selectedSkill={selectedSkill}
          selectedTarget={selectedTarget}
          isHi={isHi}
          onSelectSkill={onSelectSkill}
          onSelectTarget={onSelectTarget}
        />
      )}
    </svg>
  );
}

interface ModeProps {
  layout: LayoutResult;
  projections: ProjectionRecord[];
  selectedSkill: string | null;
  selectedTarget: string | null;
  isHi: (sid: string | null, tid: string | null) => boolean;
  onSelectSkill: (id: string) => void;
  onSelectTarget: (id: string) => void;
}

function LoomMode({ layout, projections, selectedSkill, selectedTarget, isHi, onSelectSkill, onSelectTarget }: ModeProps) {
  return (
    <>
      {layout.skills.map((s) => {
        const hi = isHi(s.id, null);
        return (
          <g key={s.id} opacity={hi ? 1 : 0.25} onClick={() => onSelectSkill(s.id)} style={{ cursor: "pointer" }}>
            <line
              x1={s.x}
              y1={s.y}
              x2={s.x}
              y2={s.y2}
              stroke={selectedSkill === s.id ? "#f4ede0" : "url(#warp-grad)"}
              strokeWidth={selectedSkill === s.id ? 2 : 1.2}
            />
            <text
              x={s.x}
              y={s.y - 16}
              textAnchor="middle"
              fontSize="10"
              fontFamily="JetBrains Mono, monospace"
              fill={selectedSkill === s.id ? "#f4ede0" : "#c9c0ae"}
              transform={`rotate(-32 ${s.x} ${s.y - 16})`}
            >
              {s.label}
            </text>
          </g>
        );
      })}

      {layout.targets.map((t) => {
        const hi = isHi(null, t.id);
        const color = ownershipColor(t.ownership);
        return (
          <g key={t.id} opacity={hi ? 1 : 0.25} onClick={() => onSelectTarget(t.id)} style={{ cursor: "pointer" }}>
            <line
              x1={t.x}
              y1={t.y}
              x2={t.x2}
              y2={t.y}
              stroke={selectedTarget === t.id ? "#f4ede0" : color}
              strokeWidth={selectedTarget === t.id ? 2 : 1.3}
              opacity={selectedTarget === t.id ? 1 : 0.55}
            />
            <text
              x={t.x - 10}
              y={t.y + 3}
              textAnchor="end"
              fontSize="10.5"
              fontFamily="JetBrains Mono, monospace"
              fill={selectedTarget === t.id ? "#f4ede0" : "#c9c0ae"}
            >
              {t.label}
            </text>
            <text
              x={layout.width - 150 + 10}
              y={t.y + 3}
              textAnchor="start"
              fontSize="9.5"
              fontFamily="JetBrains Mono, monospace"
              fill="#8a8271"
            >
              {t.ownership}
            </text>
          </g>
        );
      })}

      {projections.map((p, i) => {
        const s = layout.skills.find((x) => x.id === p.skill);
        const t = layout.targets.find((x) => x.id === p.target);
        if (!s || !t) return null;
        const hi = isHi(p.skill, p.target);
        const sel = selectedSkill === p.skill || selectedTarget === p.target;
        return (
          <g key={i} opacity={hi ? 1 : 0.15}>
            <circle
              cx={s.x}
              cy={t.y}
              r={sel ? 4.5 : 3}
              fill={methodColor(p.method)}
              stroke="#0e0d0b"
              strokeWidth="1.5"
              filter={sel ? "url(#node-glow)" : undefined}
            />
          </g>
        );
      })}
    </>
  );
}

function ForceMode({ layout, projections, selectedSkill, selectedTarget, isHi, onSelectSkill, onSelectTarget }: ModeProps) {
  return (
    <>
      {projections.map((p, i) => {
        const s = layout.skills.find((x) => x.id === p.skill);
        const t = layout.targets.find((x) => x.id === p.target);
        if (!s || !t) return null;
        const hi = isHi(p.skill, p.target);
        const mx = (s.x + t.x) / 2;
        const d = `M ${s.x} ${s.y} C ${mx} ${s.y}, ${mx} ${t.y}, ${t.x} ${t.y}`;
        return (
          <path
            key={i}
            d={d}
            stroke={methodColor(p.method)}
            strokeOpacity={hi ? 0.6 : 0.1}
            strokeWidth={hi ? 1.3 : 0.8}
            fill="none"
          />
        );
      })}
      {layout.skills.map((s) => {
        const hi = isHi(s.id, null);
        return (
          <g key={s.id} onClick={() => onSelectSkill(s.id)} style={{ cursor: "pointer" }} opacity={hi ? 1 : 0.3}>
            <rect
              x={s.x - 110}
              y={s.y - 10}
              width={108}
              height={20}
              rx={4}
              fill="#1c1915"
              stroke={selectedSkill === s.id ? "#d97736" : "#2a2620"}
            />
            <text x={s.x - 8} y={s.y + 4} textAnchor="end" fontSize="11" fontFamily="JetBrains Mono, monospace" fill="#f4ede0">
              {s.label}
            </text>
          </g>
        );
      })}
      {layout.targets.map((t) => {
        const hi = isHi(null, t.id);
        const color = ownershipColor(t.ownership);
        return (
          <g key={t.id} onClick={() => onSelectTarget(t.id)} style={{ cursor: "pointer" }} opacity={hi ? 1 : 0.3}>
            <rect
              x={t.x + 2}
              y={t.y - 10}
              width={110}
              height={20}
              rx={4}
              fill="#1c1915"
              stroke={selectedTarget === t.id ? color : "#2a2620"}
            />
            <circle cx={t.x + 12} cy={t.y} r={3} fill={color} />
            <text x={t.x + 22} y={t.y + 4} fontSize="11" fontFamily="JetBrains Mono, monospace" fill="#f4ede0">
              {t.label}
            </text>
          </g>
        );
      })}
    </>
  );
}

function TreeMode({ layout, projections, selectedSkill, selectedTarget, isHi, onSelectSkill, onSelectTarget }: ModeProps) {
  return (
    <>
      {projections.map((p, i) => {
        const s = layout.skills.find((x) => x.id === p.skill);
        const t = layout.targets.find((x) => x.id === p.target);
        if (!s || !t) return null;
        const hi = isHi(p.skill, p.target);
        const my = (s.y + t.y) / 2;
        const d = `M ${s.x} ${s.y} C ${s.x} ${my}, ${t.x} ${my}, ${t.x} ${t.y}`;
        return (
          <path
            key={i}
            d={d}
            stroke={methodColor(p.method)}
            strokeOpacity={hi ? 0.55 : 0.1}
            strokeWidth={hi ? 1.2 : 0.8}
            fill="none"
          />
        );
      })}
      {layout.skills.map((s) => {
        const hi = isHi(s.id, null);
        return (
          <g key={s.id} onClick={() => onSelectSkill(s.id)} style={{ cursor: "pointer" }} opacity={hi ? 1 : 0.3}>
            <circle
              cx={s.x}
              cy={s.y}
              r={selectedSkill === s.id ? 6 : 4}
              fill="#d97736"
              stroke="#0e0d0b"
              strokeWidth="1.5"
            />
            <text x={s.x} y={s.y - 12} textAnchor="middle" fontSize="10" fontFamily="JetBrains Mono, monospace" fill="#c9c0ae">
              {s.label}
            </text>
          </g>
        );
      })}
      {layout.targets.map((t) => {
        const hi = isHi(null, t.id);
        const color = ownershipColor(t.ownership);
        return (
          <g key={t.id} onClick={() => onSelectTarget(t.id)} style={{ cursor: "pointer" }} opacity={hi ? 1 : 0.3}>
            <rect
              x={t.x - 55}
              y={t.y - 9}
              width={110}
              height={18}
              rx={3}
              fill="#1c1915"
              stroke={selectedTarget === t.id ? color : "#2a2620"}
            />
            <text
              x={t.x}
              y={t.y + 4}
              textAnchor="middle"
              fontSize="10.5"
              fontFamily="JetBrains Mono, monospace"
              fill="#f4ede0"
            >
              {t.label}
            </text>
          </g>
        );
      })}
    </>
  );
}
