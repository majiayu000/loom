import { useState } from "react";
import { CLI_TAB_CONTENT, CLI_TAB_ORDER, type CliTabKey } from "../../lib/cli_tabs";

export function CliShowcase() {
  const [tab, setTab] = useState<CliTabKey>("quickstart");
  return (
    <section className="section" id="cli" style={{ paddingTop: 40 }}>
      <div className="container">
        <div className="section-head">
          <div className="section-eyebrow">CLI first</div>
          <h2 className="section-title">
            Script <em>anything</em>. JSON out of every command.
          </h2>
          <p className="section-sub">
            Built for automation. Every command speaks{" "}
            <code style={{ fontFamily: "var(--font-mono)", color: "var(--accent)", fontSize: 14 }}>--json</code> so you
            can pipe it into CI, git hooks, or other tools.
          </p>
        </div>

        <div className="cli-wrap">
          <div className="cli-head">
            <div className="cli-dots">
              <span />
              <span />
              <span />
            </div>
            <div className="title">~/.loom-registry — zsh</div>
          </div>
          <div className="cli-tabs">
            {CLI_TAB_ORDER.map((t) => (
              <button key={t.key} className={tab === t.key ? "active" : ""} onClick={() => setTab(t.key)}>
                {t.label}
              </button>
            ))}
          </div>
          <pre className="cli-body">{CLI_TAB_CONTENT[tab]}</pre>
        </div>
      </div>
    </section>
  );
}
