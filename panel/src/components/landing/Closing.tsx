import { LoomFooterMark } from "../icons/LoomMark";
import { GitHubIcon } from "../icons/landing_icons";

export function PullQuote() {
  return (
    <section className="pull">
      <div className="container">
        <p className="pull-text">
          "Keep <em>one</em> skill registry. Let <em>every agent</em> see its own version of the truth."
        </p>
        <div className="pull-sig">— the shape of the problem, one sentence</div>
      </div>
    </section>
  );
}

export function Cta() {
  return (
    <section className="cta" id="install">
      <div className="container">
        <h2>
          Start weaving in <em>under a minute.</em>
        </h2>
        <p>
          Clone, install, point it at your first skills directory. The panel shows up at localhost:43117.
        </p>
        <div className="cta-row">
          <a href="https://github.com/majiayu000/loom" className="btn primary lg">
            <GitHubIcon />
            View on GitHub
          </a>
          <a href="/" className="btn ghost lg">
            Open the Panel demo
          </a>
        </div>
      </div>
    </section>
  );
}

export function LandingFooter() {
  return (
    <footer>
      <div className="container">
        <div className="foot-grid">
          <div className="foot-brand">
            <div className="brand" style={{ color: "var(--ink-0)" }}>
              <LoomFooterMark />
              loom
            </div>
            <p>
              A versioned skill registry and projection control plane for AI coding agents. CLI-first. Git-native. MIT
              licensed.
            </p>
          </div>
          <div className="foot-col">
            <h5>Product</h5>
            <a href="#features">Features</a>
            <a href="#how">How it works</a>
            <a href="/">Panel</a>
            <a href="#compare">Compare</a>
          </div>
          <div className="foot-col">
            <h5>Resources</h5>
            <a href="#">CLI reference</a>
            <a href="#">Docs</a>
            <a href="#">Roadmap</a>
            <a href="#">中文指南</a>
          </div>
          <div className="foot-col">
            <h5>Community</h5>
            <a href="https://github.com/majiayu000/loom">GitHub</a>
            <a href="https://github.com/majiayu000/loom/issues">Issues</a>
            <a href="https://github.com/majiayu000/loom/discussions">Discussions</a>
            <a href="#">Changelog</a>
          </div>
        </div>
        <div className="foot-bottom">
          <span>loom v0.9.0 · built in Rust 🦀</span>
          <span>MIT · © 2026</span>
        </div>
      </div>
    </footer>
  );
}
