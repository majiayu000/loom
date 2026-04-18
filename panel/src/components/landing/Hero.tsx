import { ArrowRightIcon, ClockIcon, CopyIcon, MenuIcon, PanelIcon, StarIcon } from "../icons/landing_icons";
import { HeroViz } from "./HeroViz";

const INSTALL = "git clone https://github.com/majiayu000/loom.git && cd loom && cargo install --path .";

export function LandingHero() {
  return (
    <section className="hero">
      <div className="container hero-grid">
        <div>
          <div className="eyebrow">
            <span className="pip" />v0.9 · projection control plane
          </div>
          <h1 className="headline">
            Weave one <em>skill registry</em> across every coding agent you use.
          </h1>
          <p className="lede">
            Loom is a versioned skill registry and projection control plane for AI coding agents — Claude Code, Codex,
            Cursor, Windsurf, Cline, Copilot, Aider, OpenCode, Gemini CLI, Goose. Bind skills to targets with explicit
            rules; project as symlink, copy, or materialize; git-backed sync and replay.
          </p>
          <div className="hero-ctas">
            <a href="#install" className="btn primary lg">
              Get started
              <ArrowRightIcon />
            </a>
            <a href="/" className="btn ghost lg">
              <PanelIcon />
              Open the Panel
            </a>
          </div>
          <div className="hero-install">
            <span className="prompt">$</span>
            <span>{INSTALL}</span>
            <button
              className="copy-btn"
              aria-label="Copy install command"
              onClick={() => navigator.clipboard?.writeText(INSTALL)}
            >
              <CopyIcon />
            </button>
          </div>
          <div className="hero-meta">
            <div className="item">
              <StarIcon /> MIT licensed
            </div>
            <div className="item">
              <ClockIcon /> Written in Rust
            </div>
            <div className="item">
              <MenuIcon /> CLI + Web panel
            </div>
          </div>
        </div>
        <HeroViz />
      </div>
    </section>
  );
}
