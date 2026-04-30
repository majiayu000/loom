import type { ReactNode } from "react";
import {
  GuardIcon,
  LifecycleIcon,
  LinkIcon,
  NodeGraphIcon,
  RowsIcon,
  ShieldLargeIcon,
} from "../icons/landing_icons";

interface FeatureDef {
  icon: ReactNode;
  tag: string;
  title: string;
  body: ReactNode;
}

const FEATURES: FeatureDef[] = [
  {
    icon: <LinkIcon />,
    tag: "3 modes",
    title: "Projection",
    body: (
      <>
        Realize a skill into a target as symlink, copy, or materialize — per binding, not globally. Different agents,
        different realities.
      </>
    ),
  },
  {
    icon: <ShieldLargeIcon />,
    tag: "3 tiers",
    title: "Ownership tiers",
    body: (
      <>
        <span className="emph-managed">managed</span> — Loom writes.{" "}
        <span className="emph-observed">observed</span> — Loom reads only.{" "}
        <span className="emph-external">external</span> — hands off.
      </>
    ),
  },
  {
    icon: <RowsIcon />,
    tag: "matchers",
    title: "Binding matchers",
    body: (
      <>
        Route a skill to a target by <span className="inline-mono">path-prefix</span>,{" "}
        <span className="inline-mono">exact-path</span>, or <span className="inline-mono">name</span>. Work vs home
        Claude profiles finally behave differently.
      </>
    ),
  },
  {
    icon: <LifecycleIcon />,
    tag: "lifecycle",
    title: "Git-backed lifecycle",
    body: (
      <>
        add → capture → save → snapshot → release → rollback → diff. Every state change is an op; every op is on the
        chain.
      </>
    ),
  },
  {
    icon: <NodeGraphIcon />,
    tag: "git-backed",
    title: "Sync & replay",
    body: (
      <>
        Your team's registry is just a git repo. <span className="inline-mono">sync push / pull / replay</span> between
        machines, rollback when a capture breaks prod agents.
      </>
    ),
  },
  {
    icon: <GuardIcon />,
    tag: "safety",
    title: "Hard write guard",
    body: (
      <>
        Point <span className="inline-mono">--root</span> at the Loom repo itself? Writes are refused. No more
        accidentally overwriting the tool with the thing it's managing.
      </>
    ),
  },
];

export function Features() {
  return (
    <section className="section" id="features">
      <div className="container">
        <div className="section-head">
          <div className="section-eyebrow">What it does</div>
          <h2 className="section-title">
            Skills as <em>infrastructure</em> — not copy-pasted folders.
          </h2>
          <p className="section-sub">
            Most "skill sync" tools treat skill directories as dumb state to mirror. Loom treats them like
            infrastructure: tracked, bound, audited, replay-able.
          </p>
        </div>

        <div className="feature-grid">
          {FEATURES.map((f) => (
            <div key={f.title} className="feature">
              <div className="f-icon">{f.icon}</div>
              <div className="tag">{f.tag}</div>
              <h3>{f.title}</h3>
              <p>{f.body}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
