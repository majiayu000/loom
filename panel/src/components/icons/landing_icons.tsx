import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;
const base = { fill: "none", stroke: "currentColor", strokeLinecap: "round" as const, strokeLinejoin: "round" as const };

export const ArrowRightIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" {...base} {...p}>
    <path d="M5 12h14M13 6l6 6-6 6" />
  </svg>
);

export const PanelIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 9h18" />
  </svg>
);

export const CopyIcon = (p: IconProps) => (
  <svg width="13" height="13" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <rect x="9" y="9" width="12" height="12" rx="2" />
    <path d="M5 15V5a2 2 0 012-2h10" />
  </svg>
);

export const StarIcon = (p: IconProps) => (
  <svg width="12" height="12" viewBox="0 0 24 24" strokeWidth="1.8" {...base} {...p}>
    <path d="M12 2l2.5 7H22l-6 4.5 2.3 7L12 16l-6.3 4.5L8 13.5 2 9h7.5z" />
  </svg>
);

export const ClockIcon = (p: IconProps) => (
  <svg width="12" height="12" viewBox="0 0 24 24" strokeWidth="1.8" {...base} {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 2" />
  </svg>
);

export const MenuIcon = (p: IconProps) => (
  <svg width="12" height="12" viewBox="0 0 24 24" strokeWidth="1.8" {...base} {...p}>
    <path d="M3 7h18M3 12h18M3 17h18" />
  </svg>
);

export const LinkIcon = (p: IconProps) => (
  <svg width="20" height="20" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M9 15L6 18a3 3 0 01-4.2-4.2l3-3a3 3 0 014.2 0M15 9l3-3a3 3 0 014.2 4.2l-3 3a3 3 0 01-4.2 0" />
    <path d="M8 16l8-8" />
  </svg>
);

export const ShieldLargeIcon = (p: IconProps) => (
  <svg width="20" height="20" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M12 3l8 3v6c0 5-3.5 8.5-8 9-4.5-.5-8-4-8-9V6l8-3z" />
    <path d="M9 12l2 2 4-4" />
  </svg>
);

export const RowsIcon = (p: IconProps) => (
  <svg width="20" height="20" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M4 4h16v4H4zM4 10h10v4H4zM4 16h16v4H4z" />
  </svg>
);

export const LifecycleIcon = (p: IconProps) => (
  <svg width="20" height="20" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M3 12a9 9 0 109-9M3 4v5h5" />
    <path d="M12 7v5l3 2" />
  </svg>
);

export const NodeGraphIcon = (p: IconProps) => (
  <svg width="20" height="20" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <circle cx="6" cy="6" r="2" />
    <circle cx="6" cy="18" r="2" />
    <circle cx="18" cy="12" r="2" />
    <path d="M6 8v8M8 6h5a3 3 0 013 3v1" />
  </svg>
);

export const GuardIcon = (p: IconProps) => (
  <svg width="20" height="20" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M12 3l8 3v6c0 5-3.5 8.5-8 9-4.5-.5-8-4-8-9V6l8-3z" />
    <path d="M8 12h8M12 8v8" />
  </svg>
);

export const GitHubIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" {...p}>
    <path d="M12 1.27a11 11 0 00-3.48 21.44c.55.1.75-.23.75-.52v-1.83c-3.06.66-3.71-1.48-3.71-1.48-.5-1.28-1.22-1.62-1.22-1.62-1-.68.07-.67.07-.67 1.1.08 1.68 1.14 1.68 1.14.99 1.7 2.6 1.2 3.23.92.1-.72.39-1.2.7-1.48-2.44-.28-5.02-1.22-5.02-5.43 0-1.2.43-2.18 1.13-2.95-.11-.28-.5-1.4.11-2.92 0 0 .93-.3 3.03 1.13a10.5 10.5 0 015.52 0C14.72 4.3 15.65 4.6 15.65 4.6c.61 1.52.22 2.64.11 2.92.7.77 1.13 1.75 1.13 2.95 0 4.22-2.58 5.15-5.04 5.42.4.34.75 1 .75 2.03v3.01c0 .29.2.63.76.52A11 11 0 0012 1.27" />
  </svg>
);
