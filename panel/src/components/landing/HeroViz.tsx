const WARP_XS = [70, 110, 150, 190, 230, 270, 310, 350];
const WEFT_YS = [75, 120, 165, 210, 255, 300, 345];

const KNOTS: { x: number; y: number; color: string }[] = [
  { x: 70, y: 75, color: "#6fb78a" },
  { x: 150, y: 75, color: "#6fb78a" },
  { x: 270, y: 75, color: "#e6b450" },
  { x: 350, y: 75, color: "#6fb78a" },
  { x: 110, y: 120, color: "#6fb78a" },
  { x: 190, y: 120, color: "#c79ee0" },
  { x: 310, y: 120, color: "#6fb78a" },
  { x: 70, y: 165, color: "#e6b450" },
  { x: 230, y: 165, color: "#6fb78a" },
  { x: 270, y: 165, color: "#6fb78a" },
  { x: 150, y: 210, color: "#6fb78a" },
  { x: 190, y: 210, color: "#6fb78a" },
  { x: 310, y: 210, color: "#e6b450" },
  { x: 350, y: 210, color: "#c79ee0" },
  { x: 110, y: 255, color: "#6fb78a" },
  { x: 230, y: 255, color: "#6fb78a" },
  { x: 270, y: 255, color: "#6fb78a" },
  { x: 70, y: 300, color: "#6fb78a" },
  { x: 150, y: 300, color: "#c79ee0" },
  { x: 310, y: 300, color: "#6fb78a" },
  { x: 190, y: 345, color: "#e6b450" },
  { x: 270, y: 345, color: "#6fb78a" },
  { x: 350, y: 345, color: "#6fb78a" },
];

const SHUTTLE_PATH =
  "M30,210 L370,210 L370,255 L30,255 L30,300 L370,300 L370,345 L30,345 L30,30 L370,30 L370,75 L30,75 L30,120 L370,120 L370,165 L30,165 L30,210";

export function HeroViz() {
  return (
    <div className="hero-viz">
      <span className="corner-label tl">registry</span>
      <span className="corner-label br">targets</span>
      <svg viewBox="0 0 400 400" preserveAspectRatio="xMidYMid meet">
        <defs>
          <linearGradient id="warp-g" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#d97736" stopOpacity="0.1" />
            <stop offset="50%" stopColor="#d97736" stopOpacity="0.85" />
            <stop offset="100%" stopColor="#d97736" stopOpacity="0.1" />
          </linearGradient>
          <linearGradient id="weft-g" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0%" stopColor="#6fb78a" stopOpacity="0.15" />
            <stop offset="50%" stopColor="#6fb78a" stopOpacity="0.7" />
            <stop offset="100%" stopColor="#6fb78a" stopOpacity="0.15" />
          </linearGradient>
          <filter id="hero-glow">
            <feGaussianBlur stdDeviation="2.2" />
          </filter>
        </defs>

        <g>
          {WARP_XS.map((x) => (
            <line key={x} x1={x} y1={30} x2={x} y2={370} stroke="url(#warp-g)" strokeWidth="1.6" />
          ))}
        </g>

        <g>
          {WEFT_YS.map((y) => (
            <line key={y} x1={30} y1={y} x2={370} y2={y} stroke="url(#weft-g)" strokeWidth="1.6" />
          ))}
        </g>

        <g>
          {KNOTS.map((k, i) => (
            <circle key={i} cx={k.x} cy={k.y} r="3.5" fill={k.color} />
          ))}
        </g>

        <circle r="6" fill="#d97736" filter="url(#hero-glow)" opacity="0.95">
          <animateMotion path={SHUTTLE_PATH} dur="18s" repeatCount="indefinite" />
        </circle>

        <text
          x="200"
          y="18"
          textAnchor="middle"
          fill="#5a5346"
          fontFamily="JetBrains Mono"
          fontSize="9"
          letterSpacing="1.5"
        >
          WARP · SKILLS
        </text>
        <text
          x="12"
          y="204"
          transform="rotate(-90 12 204)"
          textAnchor="middle"
          fill="#5a5346"
          fontFamily="JetBrains Mono"
          fontSize="9"
          letterSpacing="1.5"
        >
          WEFT · TARGETS
        </text>
      </svg>
    </div>
  );
}
