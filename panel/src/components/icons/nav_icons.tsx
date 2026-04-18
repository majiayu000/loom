import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

const base = { fill: "none", stroke: "currentColor", strokeLinecap: "round" as const, strokeLinejoin: "round" as const };

export const HomeIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M3 11l9-8 9 8M5 10v10h14V10" />
  </svg>
);

export const SkillIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z" />
    <path d="M12 3v18M4 7.5l8 4.5 8-4.5" />
  </svg>
);

export const TargetIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <circle cx="12" cy="12" r="9" />
    <circle cx="12" cy="12" r="5" />
    <circle cx="12" cy="12" r="1.5" fill="currentColor" />
  </svg>
);

export const BindingIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M9 15L6 18a3 3 0 01-4.2-4.2l3-3a3 3 0 014.2 0M15 9l3-3a3 3 0 014.2 4.2l-3 3a3 3 0 01-4.2 0" />
    <path d="M8 16l8-8" />
  </svg>
);

export const OpsIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1" />
  </svg>
);

export const HistoryIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M3 12a9 9 0 109-9M3 4v5h5" />
    <path d="M12 7v5l3 2" />
  </svg>
);

export const GitIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <circle cx="6" cy="6" r="2" />
    <circle cx="6" cy="18" r="2" />
    <circle cx="18" cy="12" r="2" />
    <path d="M6 8v8M8 6h5a3 3 0 013 3v1" />
  </svg>
);

export const SettingsIcon = (p: IconProps) => (
  <svg width="16" height="16" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
  </svg>
);

export const SearchIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-4-4" />
  </svg>
);

export const PlusIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" {...base} {...p}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const PlayIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" {...p}>
    <path d="M7 5v14l11-7z" />
  </svg>
);

export const RefreshIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M20 8A8 8 0 106 18M20 3v5h-5" />
  </svg>
);

export const SyncIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M4 4v5h5M20 20v-5h-5" />
    <path d="M4 9a8 8 0 0114-3M20 15a8 8 0 01-14 3" />
  </svg>
);

export const ShieldIcon = (p: IconProps) => (
  <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="1.6" {...base} {...p}>
    <path d="M12 3l8 3v6c0 5-3.5 8.5-8 9-4.5-.5-8-4-8-9V6l8-3z" />
  </svg>
);
