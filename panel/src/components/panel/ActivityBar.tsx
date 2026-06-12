import type { ComponentType, SVGProps } from "react";
import type { PageViewModel } from "../../lib/panel_view_model";
import type { PanelPageKey } from "../../lib/types";
import {
  BindingIcon,
  GitIcon,
  HistoryIcon,
  HomeIcon,
  OpsIcon,
  SearchIcon,
  SettingsIcon,
  ShieldIcon,
  SkillIcon,
  TargetIcon,
} from "../icons/nav_icons";
import { LoomMark } from "../icons/LoomMark";

type IconComponent = ComponentType<SVGProps<SVGSVGElement>>;

const ICONS: Record<PanelPageKey, IconComponent> = {
  overview: HomeIcon,
  skills: SkillIcon,
  targets: TargetIcon,
  bindings: BindingIcon,
  projections: GitIcon,
  ops: OpsIcon,
  history: HistoryIcon,
  sync: GitIcon,
  doctor: ShieldIcon,
  settings: SettingsIcon,
};

interface ActivityBarProps {
  page: PanelPageKey;
  pages: PageViewModel[];
  onNavigate: (page: PanelPageKey) => void;
  onOpenPalette: () => void;
}

export function ActivityBar({ page, pages, onNavigate, onOpenPalette }: ActivityBarProps) {
  const buildPages = pages.filter((entry) => entry.group === "build");
  const operatePages = pages.filter((entry) => entry.group === "operate");

  return (
    <nav className="activity-bar" aria-label="Panel navigation">
      <div className="activity-brand" title="Loom">
        <LoomMark size={24} />
      </div>
      <button className="activity-command" type="button" onClick={onOpenPalette} title="Open command palette">
        <SearchIcon />
        <span className="sr-only">Command palette</span>
      </button>
      <ActivityGroup page={page} pages={buildPages} onNavigate={onNavigate} />
      <div className="activity-spacer" />
      <ActivityGroup page={page} pages={operatePages} onNavigate={onNavigate} />
    </nav>
  );
}

function ActivityGroup({
  page,
  pages,
  onNavigate,
}: {
  page: PanelPageKey;
  pages: PageViewModel[];
  onNavigate: (page: PanelPageKey) => void;
}) {
  return (
    <div className="activity-group">
      {pages.map((entry) => {
        const Icon = ICONS[entry.key];
        const active = page === entry.key;
        return (
          <button
            key={entry.key}
            className={`activity-item ${active ? "active" : ""}`}
            type="button"
            onClick={() => onNavigate(entry.key)}
            title={entry.countTitle ? `${entry.label}: ${entry.countLabel ?? entry.count}` : entry.label}
            aria-label={entry.label}
            aria-current={active ? "page" : undefined}
          >
            <Icon />
            {entry.count != null && <span className="activity-count">{entry.countLabel ?? entry.count}</span>}
          </button>
        );
      })}
    </div>
  );
}
