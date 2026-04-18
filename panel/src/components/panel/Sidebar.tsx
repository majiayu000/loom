import type { ComponentType, SVGProps } from "react";
import type { PanelPageKey } from "../../lib/types";
import {
  BindingIcon,
  GitIcon,
  HistoryIcon,
  HomeIcon,
  OpsIcon,
  SettingsIcon,
  SkillIcon,
  TargetIcon,
} from "../icons/nav_icons";

type IconComp = ComponentType<SVGProps<SVGSVGElement>>;

interface NavEntry {
  key: PanelPageKey;
  label: string;
  icon: IconComp;
  count?: number | null;
}

interface SidebarProps {
  page: PanelPageKey;
  setPage: (p: PanelPageKey) => void;
  compact: boolean;
  counts: {
    skills: number;
    targets: number;
    bindings: number;
    opsAttention: number;
  };
  registryRoot: string | null;
}

function rootLabel(root: string | null): string {
  if (!root) return "~/.loom-registry";
  return root.replace(/^\/Users\/[^/]+/, "~");
}

export function Sidebar({ page, setPage, compact, counts, registryRoot }: SidebarProps) {
  const primary: NavEntry[] = [
    { key: "overview", label: "Overview", icon: HomeIcon },
    { key: "skills", label: "Skills", icon: SkillIcon, count: counts.skills },
    { key: "targets", label: "Targets", icon: TargetIcon, count: counts.targets },
    { key: "bindings", label: "Bindings", icon: BindingIcon, count: counts.bindings },
    { key: "ops", label: "Ops", icon: OpsIcon, count: counts.opsAttention || null },
  ];
  const admin: NavEntry[] = [
    { key: "history", label: "Ops history", icon: HistoryIcon },
    { key: "sync", label: "Git sync", icon: GitIcon },
    { key: "settings", label: "Settings", icon: SettingsIcon },
  ];

  const renderItem = (n: NavEntry) => {
    const Icon = n.icon;
    return (
      <button
        key={n.key}
        className={`nav-item ${page === n.key ? "active" : ""}`}
        onClick={() => setPage(n.key)}
      >
        <Icon className="nav-icon" />
        <span className="nav-label">{n.label}</span>
        {n.count != null && <span className="nav-count">{n.count}</span>}
      </button>
    );
  };

  return (
    <div className="sidebar">
      <div className="group">
        <div className="group-label">Registry</div>
        {primary.map(renderItem)}
      </div>
      <div className="group">
        <div className="group-label">Workspace</div>
        {admin.map(renderItem)}
      </div>
      {!compact && (
        <div className="foot">
          <div style={{ color: "var(--ink-2)", marginBottom: 4 }}>{rootLabel(registryRoot)}</div>
          <div style={{ marginTop: 4 }}>loom panel</div>
        </div>
      )}
    </div>
  );
}
