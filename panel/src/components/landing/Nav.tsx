import { LoomMark } from "../icons/LoomMark";
import { StarIcon } from "../icons/landing_icons";

const LINKS = [
  { href: "#features", label: "Features" },
  { href: "#how", label: "How it works" },
  { href: "#compare", label: "Compare" },
  { href: "#cli", label: "CLI" },
  { href: "/", label: "Panel" },
];

export function LandingNav() {
  return (
    <nav className="nav">
      <div className="container nav-inner">
        <a href="#" className="brand">
          <LoomMark size={26} />
          loom
        </a>
        <div className="nav-links">
          {LINKS.map((l) => (
            <a key={l.href} href={l.href}>
              {l.label}
            </a>
          ))}
        </div>
        <div className="nav-spacer" />
        <div className="nav-cta">
          <a href="https://github.com/majiayu000/loom" className="btn">
            <StarIcon /> 0
          </a>
          <a href="#install" className="btn primary">
            Install
          </a>
        </div>
      </div>
    </nav>
  );
}
