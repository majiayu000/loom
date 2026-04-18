import type { PanelPageKey } from "../../lib/types";

const CONTENT: Partial<Record<PanelPageKey, { title: string; sub: string }>> = {
  history: {
    title: "Ops history",
    sub: "Full audit trail of every state change — diagnose, repair, replay.",
  },
  sync: {
    title: "Git sync",
    sub: "Push and pull the registry to/from your team remote. Conflicts surface here.",
  },
  settings: {
    title: "Settings",
    sub: "Registry root, agent defaults, default projection method, guard rules.",
  },
};

export function PlaceholderPage({ page }: { page: PanelPageKey }) {
  const content = CONTENT[page] ?? { title: page, sub: "" };
  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>{content.title}</h1>
          <div className="subtitle">{content.sub}</div>
        </div>
      </div>
      <div className="page-body">
        <div className="empty" style={{ padding: "80px 20px" }}>
          <div
            style={{
              fontFamily: "var(--font-display)",
              fontSize: 18,
              color: "var(--ink-2)",
              marginBottom: 8,
            }}
          >
            Not wired in this prototype
          </div>
          <div>This surface exists in the CLI — UI is on the roadmap.</div>
        </div>
      </div>
    </>
  );
}
