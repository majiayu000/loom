import { useEffect, useState, type CSSProperties } from "react";
import { api, type SkillTrashEntry } from "../../lib/api/client";
import type { Skill } from "../../lib/types";
import { useMutation } from "../../lib/useMutation";

interface SkillTrashPanelProps {
  query: string;
  readOnly: boolean;
  refreshKey: number;
  onMutation: () => void;
}

type SkillsViewMode = "skills" | "trash";

export function SkillViewModeSwitch({
  mode,
  onModeChange,
}: {
  mode: SkillsViewMode;
  onModeChange: (mode: SkillsViewMode) => void;
}) {
  return (
    <div style={modeSwitchStyle} role="group" aria-label="Skill view">
      <button style={modeSwitchButtonStyle(mode === "skills")} onClick={() => onModeChange("skills")}>
        Skills
      </button>
      <button style={modeSwitchButtonStyle(mode === "trash")} onClick={() => onModeChange("trash")}>
        Trash
      </button>
    </div>
  );
}

export function SkillTrashPanel({
  query,
  readOnly,
  refreshKey,
  onMutation,
}: SkillTrashPanelProps) {
  const [entries, setEntries] = useState<SkillTrashEntry[]>([]);
  const [selectedTrashId, setSelectedTrashId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [localRefreshKey, setLocalRefreshKey] = useState(0);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    api
      .skillTrashList(ctrl.signal)
      .then((payload) => {
        if (ctrl.signal.aborted) return;
        const nextEntries = payload.items ?? [];
        setEntries(nextEntries);
        setSelectedTrashId((current) =>
          nextEntries.some((entry) => entry.trash_id === current)
            ? current
            : nextEntries[0]?.trash_id ?? null,
        );
        setLoading(false);
      })
      .catch((err: Error) => {
        if (err.name !== "AbortError") {
          setError(err.message);
          setLoading(false);
        }
      });
    return () => ctrl.abort();
  }, [refreshKey, localRefreshKey]);

  const filtered = filterTrashEntries(entries, query);
  const selected =
    filtered.find((entry) => entry.trash_id === selectedTrashId) ?? filtered[0] ?? null;

  const refreshTrash = () => {
    setLocalRefreshKey((value) => value + 1);
    onMutation();
  };

  return (
    <>
      {(loading || error) && (
        <div style={statusBarStyle(error ? "err" : "muted")}>
          {error ?? (entries.length === 0 ? "Loading trash..." : "Refreshing trash...")}
        </div>
      )}
      <div className="two-col" style={{ height: "100%", gap: 0 }}>
        <div style={{ overflow: "auto", borderRight: "1px solid var(--line)" }}>
          <table className="tbl">
            <thead>
              <tr>
                <th>Skill</th>
                <th>Trashed</th>
                <th>Source commit</th>
                <th>Trash id</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((entry) => (
                <tr
                  key={entry.trash_id}
                  className={selected?.trash_id === entry.trash_id ? "selected" : ""}
                  onClick={() => setSelectedTrashId(entry.trash_id)}
                >
                  <td className="name">{entry.skill}</td>
                  <td className="mono dim">{formatTrashTime(entry.trashed_at)}</td>
                  <td className="mono">{shortCommit(entry.source_commit)}</td>
                  <td className="mono dim">{entry.trash_id}</td>
                </tr>
              ))}
              {filtered.length === 0 && (
                <tr>
                  <td colSpan={4} style={{ color: "var(--ink-3)", padding: 22, textAlign: "center" }}>
                    {query ? "No trash entries match the current filter." : "Trash is empty."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <div style={{ padding: 20, overflow: "auto" }}>
          {selected ? (
            <TrashEntryDetail entry={selected} readOnly={readOnly} onSuccess={refreshTrash} />
          ) : (
            <div className="empty">{query ? "No trash entries match the current filter." : "Trash is empty."}</div>
          )}
        </div>
      </div>
    </>
  );
}

interface TrashSkillActionProps {
  skill: Skill;
  readOnly: boolean;
  onSuccess: () => void;
}

export function TrashSkillAction({ skill, readOnly, onSuccess }: TrashSkillActionProps) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const trash = useMutation();
  const disabled = readOnly || skill.sourceStatus !== "present" || trash.busy;
  const title = readOnly
    ? "registry offline"
    : skill.sourceStatus !== "present"
      ? "only present source skills can be moved to trash"
      : undefined;

  const moveToTrash = () => {
    trash.run(`trash ${skill.name}`, () => api.skillTrashAdd(skill.name), () => {
      setConfirmOpen(false);
      onSuccess();
    });
  };

  return (
    <div style={{ display: "grid", gap: 8, margin: "0 0 14px" }}>
      <div style={{ display: "flex", justifyContent: "flex-end" }}>
        <button
          className="btn ghost danger"
          aria-label={`Trash ${skill.name}`}
          onClick={() => setConfirmOpen((value) => !value)}
          disabled={disabled}
          title={title}
        >
          {trash.busy ? "trashing..." : "Trash"}
        </button>
      </div>
      {confirmOpen && (
        <div className="card" style={{ padding: 12 }}>
          <div style={{ color: "var(--ink-1)", fontSize: 12.5 }}>
            Move <span className="mono">{skill.name}</span> to Git-tracked trash?
          </div>
          <div className="mono" style={{ color: "var(--ink-3)", fontSize: 11, marginTop: 4 }}>
            It can be restored later from the Trash view.
          </div>
          {(trash.error || trash.success) && (
            <div style={trash.error ? errorStyle : okStyle}>
              {trash.error ?? `✓ ${trash.success}`}
            </div>
          )}
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 12 }}>
            <button className="btn ghost" onClick={() => setConfirmOpen(false)} disabled={trash.busy}>
              Cancel
            </button>
            <button className="btn ghost danger" onClick={moveToTrash} disabled={trash.busy}>
              Move to trash
            </button>
          </div>
        </div>
      )}
      {!confirmOpen && (trash.error || trash.success) && (
        <div style={trash.error ? errorStyle : okStyle}>{trash.error ?? `✓ ${trash.success}`}</div>
      )}
    </div>
  );
}

function TrashEntryDetail({
  entry,
  readOnly,
  onSuccess,
}: {
  entry: SkillTrashEntry;
  readOnly: boolean;
  onSuccess: () => void;
}) {
  const [confirmPurge, setConfirmPurge] = useState(false);
  const restore = useMutation();
  const purge = useMutation();

  const runRestore = () => {
    restore.run(
      `restore ${entry.skill}`,
      () => api.skillTrashRestore(entry.trash_id, { skill: entry.skill }),
      onSuccess,
    );
  };

  const runPurge = () => {
    purge.run(`purge ${entry.trash_id}`, () => api.skillTrashPurge(entry.trash_id), () => {
      setConfirmPurge(false);
      onSuccess();
    });
  };

  return (
    <div className="detail">
      <h4>{entry.skill}</h4>
      <div className="dpath">{entry.trash_path}</div>
      <div className="kv">
        <div className="k">Original path</div>
        <div className="v">{entry.original_path}</div>
        <div className="k">Trashed</div>
        <div className="v">{formatTrashTime(entry.trashed_at)}</div>
        <div className="k">Source commit</div>
        <div className="v">{shortCommit(entry.source_commit)}</div>
        <div className="k">Trash id</div>
        <div className="v">{entry.trash_id}</div>
      </div>

      <div className="card" style={{ padding: 12, marginTop: 14 }}>
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button
            className="btn primary"
            onClick={runRestore}
            disabled={readOnly || restore.busy || purge.busy}
            title={readOnly ? "registry offline" : undefined}
          >
            {restore.busy ? "restoring..." : "Restore"}
          </button>
          <button
            className="btn ghost danger"
            onClick={() => setConfirmPurge((value) => !value)}
            disabled={readOnly || restore.busy || purge.busy}
            title={readOnly ? "registry offline" : undefined}
          >
            Purge
          </button>
        </div>
        {confirmPurge && (
          <div style={{ borderTop: "1px solid var(--line)", marginTop: 12, paddingTop: 12 }}>
            <div style={{ color: "var(--ink-1)", fontSize: 12.5 }}>
              Permanently remove this trash entry?
            </div>
            <div className="mono" style={{ color: "var(--ink-3)", fontSize: 11, marginTop: 4 }}>
              This only purges <span>{entry.trash_id}</span>.
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 12 }}>
              <button className="btn ghost" onClick={() => setConfirmPurge(false)} disabled={purge.busy}>
                Cancel
              </button>
              <button className="btn ghost danger" onClick={runPurge} disabled={purge.busy}>
                {purge.busy ? "purging..." : "Purge forever"}
              </button>
            </div>
          </div>
        )}
        {(restore.error || restore.success || purge.error || purge.success) && (
          <div style={restore.error || purge.error ? errorStyle : okStyle}>
            {restore.error ?? purge.error ?? `✓ ${restore.success ?? purge.success}`}
          </div>
        )}
      </div>
    </div>
  );
}

function filterTrashEntries(entries: SkillTrashEntry[], query: string): SkillTrashEntry[] {
  if (!query) return entries;
  return entries.filter((entry) =>
    [entry.skill, entry.trash_id, entry.original_path, entry.source_commit].some((value) =>
      value.toLowerCase().includes(query),
    ),
  );
}

function shortCommit(value: string): string {
  return value.length > 8 ? value.slice(0, 8) : value;
}

function formatTrashTime(value: string): string {
  const parsed = Date.parse(value);
  if (Number.isNaN(parsed)) return value;
  return new Date(parsed).toLocaleString();
}

function statusBarStyle(tone: "err" | "muted"): CSSProperties {
  return {
    padding: "6px 28px",
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    borderBottom: "1px solid var(--line)",
    color: tone === "err" ? "var(--err)" : "var(--ink-3)",
    background: tone === "err" ? "rgba(216,90,90,0.08)" : "var(--bg-1)",
  };
}

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

const modeSwitchStyle: CSSProperties = {
  display: "inline-flex",
  height: 32,
  border: "1px solid var(--line-hi)",
  borderRadius: 6,
  overflow: "hidden",
  background: "var(--bg)",
};

function modeSwitchButtonStyle(active: boolean): CSSProperties {
  return {
    minWidth: 62,
    padding: "0 10px",
    color: active ? "var(--ink-0)" : "var(--ink-3)",
    background: active ? "var(--bg-2)" : "transparent",
    borderRight: "1px solid var(--line)",
    fontSize: 12,
  };
}
