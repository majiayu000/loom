import { useEffect, useState } from "react";
import type { InfoPayload } from "../../types";
import { api, ApiError } from "../../lib/api/client";

interface SettingsPageProps {
  live: boolean;
  registryRoot: string | null;
}

type InfoState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; info: InfoPayload }
  | { kind: "error"; message: string };

export function SettingsPage({ live, registryRoot }: SettingsPageProps) {
  const [info, setInfo] = useState<InfoState>({ kind: "idle" });
  const [cleared, setCleared] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    setInfo({ kind: "loading" });
    api
      .info(controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return;
        setInfo({ kind: "ready", info: payload });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
        setInfo({ kind: "error", message });
      });
    return () => controller.abort();
  }, [live]);

  const resetTweaks = () => {
    localStorage.removeItem("loom.tweaks");
    setCleared(true);
    window.setTimeout(() => window.location.reload(), 400);
  };

  const rows: Array<{ label: string; value: string | undefined; mono?: boolean }> = [
    { label: "Registry root", value: registryRoot ?? undefined, mono: true },
  ];
  if (info.kind === "ready") {
    const x = info.info;
    rows.push(
      { label: "State dir", value: x.state_dir, mono: true },
      { label: "V3 targets file", value: x.v3_targets_file, mono: true },
      { label: "Claude dir", value: x.claude_dir, mono: true },
      { label: "Codex dir", value: x.codex_dir, mono: true },
      { label: "Remote URL", value: x.remote_url, mono: true },
    );
  }

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Settings</h1>
          <div className="subtitle">
            Where Loom keeps its state. These paths come from <span className="mono">/api/info</span> and mirror the CLI
            output of <span className="mono">loom info</span>.
          </div>
        </div>
      </div>
      <div className="page-body">
        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-head">
            <h3>Registry paths</h3>
            {info.kind === "loading" && <span className="chip">loading…</span>}
            {info.kind === "error" && <span className="chip" style={{ color: "var(--err)" }}>fetch failed</span>}
          </div>
          <div className="card-body">
            {info.kind === "error" && (
              <div style={{ color: "var(--err)", fontSize: 12, marginBottom: 10 }}>{info.message}</div>
            )}
            <table className="tbl" style={{ fontSize: 12 }}>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.label}>
                    <td style={{ color: "var(--ink-2)", width: 160 }}>{r.label}</td>
                    <td className={r.mono ? "mono" : undefined} style={{ color: r.value ? "var(--ink-0)" : "var(--ink-3)" }}>
                      {r.value ?? "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <h3>UI preferences</h3>
            {cleared && <span className="chip ok">cleared · reloading…</span>}
          </div>
          <div className="card-body" style={{ fontSize: 12 }}>
            <div style={{ marginBottom: 10, color: "var(--ink-2)" }}>
              Viz mode, accent, density, font and compact toggle live in{" "}
              <span className="mono">localStorage.loom.tweaks</span>. Click below to reset to defaults and reload.
            </div>
            <button className="btn" onClick={resetTweaks} disabled={cleared}>
              Reset UI preferences
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
