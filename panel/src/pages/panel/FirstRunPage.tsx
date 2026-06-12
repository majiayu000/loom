import { useState } from "react";
import { api, ApiError, type CommandEnvelope } from "../../lib/api/client";

interface FirstRunPageProps {
  registryRoot: string | null;
  onReady: () => void;
}

function countArray(data: Record<string, unknown> | undefined, key: string): number {
  const value = data?.[key];
  return Array.isArray(value) ? value.length : 0;
}

function resultText(envelope: CommandEnvelope | null): string | null {
  if (!envelope) return null;
  const imported = countArray(envelope.data, "imported");
  const skipped = countArray(envelope.data, "skipped");
  return `Initialized. ${imported} observed targets imported, ${skipped} paths skipped.`;
}

export function FirstRunPage({ registryRoot, onReady }: FirstRunPageProps) {
  const [scanExisting, setScanExisting] = useState(true);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<CommandEnvelope | null>(null);

  const initialize = async () => {
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      const envelope = await api.workspaceInit({ scan_existing: scanExisting });
      setResult(envelope);
      onReady();
    } catch (err) {
      const message = err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err);
      setError(message);
    } finally {
      setRunning(false);
    }
  };

  return (
    <>
      <div className="page-header">
        <div className="title-block">
          <h1>Initialize Registry</h1>
          <div className="subtitle">
            {registryRoot ?? "Registry root not connected"} needs state before Loom can show targets and bindings.
          </div>
        </div>
      </div>
      <div className="page-body">
        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-head">
            <h3>First run</h3>
            {running && <span className="chip">initializing...</span>}
            {result && <span className="chip ok">ready</span>}
          </div>
          <div className="card-body">
            <div className="kpi-row" style={{ marginBottom: 14 }}>
              <FirstRunKpi label="Registry root" value={registryRoot ?? "not connected"} />
              <FirstRunKpi label="Scan mode" value={scanExisting ? "scan existing dirs" : "initialize only"} />
              <FirstRunKpi label="State" value={running ? "running" : result ? "ready" : "waiting"} />
            </div>
            <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 14 }}>
              <input
                type="checkbox"
                checked={scanExisting}
                onChange={(event) => setScanExisting(event.currentTarget.checked)}
              />
              <span>Scan existing agent skill directories</span>
            </label>
            {error && <div style={{ color: "var(--err)", fontSize: 12, marginBottom: 12 }}>{error}</div>}
            {resultText(result) && (
              <div style={{ color: "var(--ok)", fontSize: 12, marginBottom: 12 }}>{resultText(result)}</div>
            )}
            <button className="btn primary" onClick={initialize} disabled={running}>
              {running ? "Initializing..." : "Initialize"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

function FirstRunKpi({ label, value }: { label: string; value: string }) {
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value status-value">{value}</div>
    </div>
  );
}
