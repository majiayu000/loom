interface Row {
  capability: string;
  skillsHub: string | { kind: "check" | "dash" | "dim"; label?: string };
  ccSwitch: string | { kind: "check" | "dash" | "dim"; label?: string };
  agentSkills: string | { kind: "check" | "dash" | "dim"; label?: string };
  loom: { kind: "check" | "dim"; label?: string };
}

const ROWS: Row[] = [
  { capability: "Projection: symlink", skillsHub: { kind: "check" }, ccSwitch: { kind: "check" }, agentSkills: { kind: "check" }, loom: { kind: "check" } },
  { capability: "Projection: copy", skillsHub: { kind: "check" }, ccSwitch: { kind: "check" }, agentSkills: { kind: "check" }, loom: { kind: "check" } },
  { capability: "Projection: materialize", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dash" }, loom: { kind: "check" } },
  { capability: "Ownership tiers", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dash" }, loom: { kind: "check" } },
  { capability: "Binding matchers", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dash" }, loom: { kind: "check" } },
  { capability: "Profiles (multi-config per agent)", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dash" }, loom: { kind: "check" } },
  { capability: "Snapshot / rollback / diff", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dim", label: "lockfile" }, loom: { kind: "check" } },
  { capability: "Ops history · diagnose · repair", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dim", label: "audit logs" }, loom: { kind: "check" } },
  { capability: "Git-native sync + replay", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dim", label: "cloud sync" }, agentSkills: { kind: "dash" }, loom: { kind: "check" } },
  { capability: "Hard write guard", skillsHub: { kind: "dash" }, ccSwitch: { kind: "dash" }, agentSkills: { kind: "dash" }, loom: { kind: "check" } },
  { capability: "CLI-first + Web panel", skillsHub: { kind: "dim", label: "GUI only" }, ccSwitch: { kind: "dim", label: "GUI only" }, agentSkills: { kind: "dim", label: "CLI only" }, loom: { kind: "check" } },
  { capability: "Desktop app (dmg/msi)", skillsHub: { kind: "check" }, ccSwitch: { kind: "check" }, agentSkills: { kind: "dash" }, loom: { kind: "dim", label: "roadmap" } },
  { capability: "Agents supported", skillsHub: "44", ccSwitch: "5", agentSkills: "18", loom: { kind: "check", label: "10" } },
];

function renderCell(cell: Row["skillsHub"]) {
  if (typeof cell === "string") return <>{cell}</>;
  if (cell.kind === "check") return <span className="check">{cell.label ?? "✓"}</span>;
  if (cell.kind === "dash") return <span className="dash">—</span>;
  return <span className="dim">{cell.label ?? "—"}</span>;
}

function renderLoomCell(cell: Row["loom"]) {
  if (cell.kind === "check") return cell.label ?? "✓";
  return cell.label ?? "—";
}

export function Compare() {
  return (
    <section className="section" id="compare" style={{ paddingTop: 40 }}>
      <div className="container">
        <div className="section-head">
          <div className="section-eyebrow">Compared</div>
          <h2 className="section-title">
            Pick Loom when you want <em>fine-grained control</em>.
          </h2>
          <p className="section-sub">
            Other skill managers are GUI-first and mirror skills one-way. Loom adds binding semantics, versioning, and
            audit.
          </p>
        </div>

        <div className="compare-table">
          <table>
            <thead>
              <tr>
                <th>Capability</th>
                <th>skills-hub</th>
                <th>cc-switch</th>
                <th>agent-skills</th>
                <th className="highlight">Loom</th>
              </tr>
            </thead>
            <tbody>
              {ROWS.map((r) => (
                <tr key={r.capability}>
                  <td>{r.capability}</td>
                  <td>{renderCell(r.skillsHub)}</td>
                  <td>{renderCell(r.ccSwitch)}</td>
                  <td>{renderCell(r.agentSkills)}</td>
                  <td className={`loom-col${r.loom.kind === "dim" ? " dim" : ""}`}>{renderLoomCell(r.loom)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <p className="compare-note">
          If you want a one-click GUI with broad agent coverage and don't need projection/binding semantics,{" "}
          <a href="#">skills-hub</a> or <a href="#">cc-switch</a> are great picks too.
        </p>
      </div>
    </section>
  );
}
