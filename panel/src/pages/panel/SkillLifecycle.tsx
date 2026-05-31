import { useEffect, useState, type CSSProperties, type FormEvent } from "react";
import { api, type RegistryObservationEvent, type SkillDiffFile } from "../../lib/api/client";
import { useMutation } from "../../lib/useMutation";

export interface LifecycleEvent {
  kind: "release" | "capture" | "save" | "snapshot" | "project" | "rollback";
  v: string;
  time: string;
  who: string;
  desc: string;
}

const KIND_COLOR: Record<LifecycleEvent["kind"], string> = {
  release: "var(--accent)",
  capture: "var(--pending)",
  save: "var(--ink-2)",
  snapshot: "var(--warn)",
  project: "var(--ok)",
  rollback: "var(--err)",
};

const KIND_MAP: Record<string, LifecycleEvent["kind"]> = {
  captured: "capture",
  projected: "project",
  rollback: "rollback",
  monitor: "save",
  snapshot: "snapshot",
  released: "release",
  saved: "save",
  file_changed: "save",
  health_changed: "snapshot",
};

export function mapObsToLifecycle(ev: RegistryObservationEvent): LifecycleEvent {
  return {
    kind: KIND_MAP[ev.kind] ?? "capture",
    v: ev.event_id.slice(0, 8),
    time: toRelative(ev.observed_at),
    who: ev.instance_id.slice(0, 8),
    desc: ev.path ?? (ev.from && ev.to ? `${ev.from} -> ${ev.to}` : ev.kind),
  };
}

export function LifecycleActions({
  skillName,
  onMutation,
  readOnly,
}: {
  skillName: string;
  onMutation: () => void;
  readOnly: boolean;
}) {
  const [version, setVersion] = useState("");
  const [rollbackRef, setRollbackRef] = useState("");
  const save = useMutation();
  const snapshot = useMutation();
  const release = useMutation();
  const rollback = useMutation();

  const runSave = () => {
    save.run("skill save", () => api.skillSave(skillName), onMutation);
  };

  const runSnapshot = () => {
    snapshot.run("skill snapshot", () => api.skillSnapshot(skillName), onMutation);
  };

  const submitRelease = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = version.trim();
    if (!trimmed) return;
    release.run("skill release", () => api.skillRelease(skillName, { version: trimmed }), () => {
      setVersion("");
      onMutation();
    });
  };

  const submitRollback = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = rollbackRef.trim();
    rollback.run(
      "skill rollback",
      () => api.skillRollback(skillName, trimmed ? { to: trimmed } : {}),
      () => {
        setRollbackRef("");
        onMutation();
      },
    );
  };

  const busy = save.busy || snapshot.busy || release.busy || rollback.busy;
  const disabled = readOnly || busy;
  const status =
    save.error ??
    snapshot.error ??
    release.error ??
    rollback.error ??
    save.success ??
    snapshot.success ??
    release.success ??
    rollback.success;
  const hasError = Boolean(save.error ?? snapshot.error ?? release.error ?? rollback.error);

  return (
    <div className="card" style={{ padding: 12, margin: "14px 0" }}>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(210px, 1fr))", gap: 10 }}>
        <button
          className="btn ghost"
          onClick={runSave}
          disabled={disabled}
          title={readOnly ? "registry offline" : undefined}
          style={fullWidthButtonStyle}
        >
          {save.busy ? "saving..." : "Save"}
        </button>
        <button
          className="btn ghost"
          onClick={runSnapshot}
          disabled={disabled}
          title={readOnly ? "registry offline" : undefined}
          style={fullWidthButtonStyle}
        >
          {snapshot.busy ? "snapshotting..." : "Snapshot"}
        </button>
        <form onSubmit={submitRelease} style={{ display: "flex", gap: 8, minWidth: 0 }}>
          <input
            value={version}
            onChange={(event) => setVersion(event.target.value)}
            placeholder="version"
            style={formInputStyle}
            disabled={disabled}
          />
          <button className="btn primary" type="submit" disabled={disabled || !version.trim()}>
            {release.busy ? "releasing..." : "Release"}
          </button>
        </form>
        <form onSubmit={submitRollback} style={{ display: "flex", gap: 8, minWidth: 0 }}>
          <input
            value={rollbackRef}
            onChange={(event) => setRollbackRef(event.target.value)}
            placeholder="HEAD~1"
            style={formInputStyle}
            disabled={disabled}
          />
          <button className="btn ghost danger" type="submit" disabled={disabled}>
            {rollback.busy ? "rolling back..." : "Rollback"}
          </button>
        </form>
      </div>
      {status && <div style={hasError ? errorStyle : okStyle}>{hasError ? status : `✓ ${status}`}</div>}
    </div>
  );
}

export function Lifecycle({ events, skillName }: { events: LifecycleEvent[]; skillName: string }) {
  if (events.length === 0) {
    return (
      <div style={{ padding: "18px 4px", fontSize: 12, color: "var(--ink-2)" }}>
        <div style={{ marginBottom: 6 }}>No lifecycle events yet.</div>
        <div className="mono" style={{ fontSize: 11, color: "var(--ink-3)" }}>
          Run <span style={{ color: "var(--ink-1)" }}>loom capture {skillName}</span> to start the chain.
        </div>
      </div>
    );
  }
  return (
    <div style={{ position: "relative", paddingLeft: 22 }}>
      <div style={{ position: "absolute", left: 7, top: 4, bottom: 4, width: 1, background: "var(--line)" }} />
      {events.map((e, i) => (
        <div key={i} style={{ position: "relative", marginBottom: 14 }}>
          <div
            style={{
              position: "absolute",
              left: -22,
              top: 4,
              width: 15,
              height: 15,
              borderRadius: 8,
              background: "var(--bg-0)",
              border: `2px solid ${KIND_COLOR[e.kind]}`,
            }}
          />
          <div style={{ fontSize: 12 }}>
            <span style={{ color: "var(--ink-0)", fontWeight: 500 }}>{e.kind}</span>
            <span className="mono" style={{ color: "var(--ink-2)", marginLeft: 6 }}>
              {e.v}
            </span>
            <span style={{ color: "var(--ink-3)", marginLeft: 8 }}>
              by {e.who} · {e.time}
            </span>
          </div>
          <div style={{ fontSize: 11.5, color: "var(--ink-2)", marginTop: 2 }}>{e.desc}</div>
        </div>
      ))}
    </div>
  );
}

export function SkillDiff({ skillName }: { skillName: string }) {
  const [revA, setRevA] = useState("");
  const [revB, setRevB] = useState("");
  const [files, setFiles] = useState<SkillDiffFile[] | null>(null);
  const [header, setHeader] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    api
      .skillDiff(skillName, revA || undefined, revB || undefined, ctrl.signal)
      .then((payload) => {
        if (payload.data) {
          setFiles(payload.data.files);
          setHeader(`${payload.data.rev_a.slice(0, 7)} -> ${payload.data.rev_b.slice(0, 7)}`);
        }
        setLoading(false);
      })
      .catch((err: Error) => {
        if (err.name !== "AbortError") {
          setError(err.message);
          setFiles(null);
          setHeader("");
          setLoading(false);
        }
      });
    return () => ctrl.abort();
  }, [skillName, revA, revB]);

  const inputStyle: CSSProperties = {
    fontSize: 11,
    padding: "2px 6px",
    background: "var(--bg-1)",
    border: "1px solid var(--line)",
    borderRadius: 4,
    color: "var(--ink-0)",
    width: 130,
    fontFamily: "var(--font-mono)",
  };

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <input
          style={inputStyle}
          placeholder="rev_a"
          value={revA}
          onChange={(e) => setRevA(e.target.value)}
        />
        <span style={{ color: "var(--ink-3)", fontSize: 11 }}>{"->"}</span>
        <input
          style={inputStyle}
          placeholder="rev_b"
          value={revB}
          onChange={(e) => setRevB(e.target.value)}
        />
      </div>

      {loading && <div style={{ color: "var(--ink-3)", fontSize: 12 }}>Loading...</div>}
      {error && (
        <div style={{ color: "var(--err)", fontSize: 11, fontFamily: "var(--font-mono)" }}>
          {error}
        </div>
      )}

      {!loading && files !== null && (
        <>
          {header && <div className="section-title">{header}</div>}
          {files.length === 0 ? (
            <div style={{ color: "var(--ink-3)", fontSize: 12 }}>
              No changes in skills/{skillName}/
            </div>
          ) : (
            files.map((file) => (
              <div key={file.path} style={{ marginBottom: 16 }}>
                <div
                  className="mono"
                  style={{ fontSize: 11, color: "var(--ink-2)", marginBottom: 4 }}
                >
                  {file.path}{" "}
                  <span style={{ color: "var(--ok)" }}>+{file.added}</span>{" "}
                  <span style={{ color: "var(--err)" }}>-{file.removed}</span>
                  {file.truncated && (
                    <span
                      title={
                        `${file.truncated_lines ?? 0} hidden diff line(s); narrow the revision range.`
                      }
                      style={{
                        marginLeft: 8,
                        padding: "0 6px",
                        borderRadius: 3,
                        background: "var(--bg-2)",
                        color: "var(--warn, var(--ink-3))",
                        fontSize: 10,
                      }}
                    >
                      truncated +{file.truncated_lines ?? 0}
                    </span>
                  )}
                </div>
                <div style={{ border: "1px solid var(--line)", borderRadius: 6, overflow: "hidden" }}>
                  {file.hunks.map((hunk, hi) => (
                    <div key={hi}>
                      <div className="diff-row" style={{ background: "var(--bg-1)" }}>
                        <div className="mark" />
                        <div className="l" style={{ color: "var(--ink-3)" }}>{hunk.header}</div>
                      </div>
                      {hunk.lines.map((line, li) => (
                        <div
                          key={li}
                          className={`diff-row${line.startsWith("+") ? " add" : line.startsWith("-") ? " del" : ""}`}
                        >
                          <div className="mark">
                            {line.startsWith("+") ? "+" : line.startsWith("-") ? "-" : ""}
                          </div>
                          <div className="l">{line.slice(1)}</div>
                        </div>
                      ))}
                    </div>
                  ))}
                </div>
              </div>
            ))
          )}
        </>
      )}
    </div>
  );
}

function toRelative(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

const formInputStyle: CSSProperties = {
  padding: "6px 10px",
  borderRadius: 6,
  border: "1px solid var(--line-hi)",
  background: "var(--bg-2)",
  color: "var(--ink-0)",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  minWidth: 0,
};

const fullWidthButtonStyle: CSSProperties = {
  width: "100%",
  justifyContent: "center",
};

const errorStyle: CSSProperties = {
  marginTop: 10,
  padding: "6px 10px",
  color: "var(--err)",
  background: "rgba(216,90,90,0.08)",
  border: "1px solid rgba(216,90,90,0.3)",
  borderRadius: 6,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
};

const okStyle: CSSProperties = {
  ...errorStyle,
  color: "var(--ok)",
  background: "rgba(111,183,138,0.08)",
  border: "1px solid rgba(111,183,138,0.3)",
};
