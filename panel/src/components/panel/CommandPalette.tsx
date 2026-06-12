import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent } from "react";
import type { PanelViewModel } from "../../lib/panel_view_model";
import type { PanelPageKey } from "../../lib/types";
import { SearchIcon } from "../icons/nav_icons";

interface CommandPaletteProps {
  open: boolean;
  viewModel: PanelViewModel;
  onClose: () => void;
  onNavigate: (page: PanelPageKey) => void;
  onSelectSkill: (id: string) => void;
  onSelectTarget: (id: string) => void;
  onReplayQueued: () => Promise<void> | void;
}

interface PaletteCommand {
  id: string;
  group: string;
  label: string;
  detail?: string;
  disabled?: boolean;
  disabledReason?: string;
  run: () => Promise<void> | void;
}

export function CommandPalette({
  open,
  viewModel,
  onClose,
  onNavigate,
  onSelectSkill,
  onSelectTarget,
  onReplayQueued,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const commands = useMemo<PaletteCommand[]>(() => {
    const pageCommands = viewModel.shell.pages.map((page) => ({
      id: `page:${page.key}`,
      group: "Pages",
      label: page.label,
      detail: page.key,
      run: () => onNavigate(page.key),
    }));
    const skillCommands = viewModel.skills.map((skill) => ({
      id: `skill:${skill.id}`,
      group: "Skills",
      label: skill.name.label,
      detail: skill.description.state === "available" ? skill.description.label : "Open skill",
      disabled: skill.name.state === "unavailable",
      disabledReason: skill.name.title,
      run: () => onSelectSkill(skill.id),
    }));
    const targetCommands = viewModel.targets.map((target) => ({
      id: `target:${target.id}`,
      group: "Targets",
      label: target.id,
      detail: target.path.state === "available" ? target.path.label : target.agent.label,
      run: () => onSelectTarget(target.id),
    }));
    const replay = viewModel.actions.replayQueued;
    const mutationCommands =
      viewModel.shell.counts.queuedWrites.value && viewModel.shell.counts.queuedWrites.value > 0
        ? [
            {
              id: "mutation:replayQueued",
              group: "Commands",
              label: replay.label,
              detail: replay.disabledReason,
              disabled: !replay.enabled,
              disabledReason: replay.disabledReason,
              run: onReplayQueued,
            },
          ]
        : [];
    return [...pageCommands, ...skillCommands, ...targetCommands, ...mutationCommands];
  }, [onNavigate, onReplayQueued, onSelectSkill, onSelectTarget, viewModel]);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return commands;
    return commands.filter((command) =>
      [command.group, command.label, command.detail ?? ""].some((value) => value.toLowerCase().includes(needle)),
    );
  }, [commands, query]);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
    window.setTimeout(() => inputRef.current?.focus(), 0);
  }, [open]);

  if (!open) return null;

  const runCommand = async (command: PaletteCommand) => {
    if (command.disabled) return;
    await command.run();
    onClose();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((index) => Math.min(filtered.length - 1, index + 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((index) => Math.max(0, index - 1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const command = filtered[activeIndex];
      if (command) void runCommand(command);
    }
  };

  return (
    <div className="palette-backdrop" role="presentation" onMouseDown={onClose} style={paletteStyles.backdrop}>
      <div
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
        style={paletteStyles.dialog}
      >
        <div className="palette-input-row" style={paletteStyles.inputRow}>
          <SearchIcon style={paletteStyles.icon} />
          <input
            ref={inputRef}
            role="searchbox"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={onKeyDown}
            placeholder="Search pages, skills, targets"
            style={paletteStyles.input}
          />
          <kbd style={paletteStyles.kbd}>Esc</kbd>
        </div>
        <div className="palette-results" role="listbox" aria-label="Command results" style={paletteStyles.results}>
          {filtered.length === 0 ? (
            <div className="palette-empty" style={paletteStyles.empty}>No commands found.</div>
          ) : (
            filtered.map((command, index) => (
              <button
                key={command.id}
                className={`palette-item ${index === activeIndex ? "active" : ""}`}
                type="button"
                role="option"
                aria-selected={index === activeIndex}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => void runCommand(command)}
                disabled={command.disabled}
                title={command.disabledReason}
                style={{
                  ...paletteStyles.item,
                  ...(index === activeIndex ? paletteStyles.itemActive : null),
                  ...(command.disabled ? paletteStyles.itemDisabled : null),
                }}
              >
                <span className="palette-group" style={paletteStyles.group}>{command.group}</span>
                <span className="palette-label" style={paletteStyles.label}>{command.label}</span>
                {command.detail && <span className="palette-detail" style={paletteStyles.detail}>{command.detail}</span>}
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

const clippedText: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const paletteStyles = {
  backdrop: {
    position: "fixed",
    inset: 0,
    zIndex: 90,
    display: "flex",
    alignItems: "flex-start",
    justifyContent: "center",
    paddingTop: "12vh",
    background: "rgba(0, 0, 0, 0.42)",
  },
  dialog: {
    width: "min(640px, calc(100vw - 24px))",
    maxHeight: "min(620px, 76vh)",
    display: "flex",
    flexDirection: "column",
    border: "1px solid var(--line-hi)",
    borderRadius: "var(--radius)",
    background: "var(--bg-1)",
    boxShadow: "var(--shadow)",
    overflow: "hidden",
  },
  inputRow: {
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "12px 14px",
    borderBottom: "1px solid var(--line)",
  },
  icon: { width: 18, height: 18, color: "var(--ink-2)" },
  input: {
    flex: 1,
    minWidth: 0,
    color: "var(--ink-0)",
    background: "transparent",
    border: 0,
    outline: 0,
    fontSize: 14,
  },
  kbd: {
    color: "var(--ink-3)",
    fontFamily: "var(--font-mono)",
    fontSize: 10,
    border: "1px solid var(--line-hi)",
    borderRadius: "var(--radius-sm)",
    padding: "1px 5px",
  },
  results: { overflow: "auto", padding: 6 },
  item: {
    width: "100%",
    minHeight: 46,
    display: "grid",
    gridTemplateColumns: "92px minmax(0, 1fr)",
    gap: "4px 10px",
    alignItems: "center",
    padding: "8px 10px",
    borderRadius: "var(--radius-sm)",
    color: "var(--ink-1)",
    textAlign: "left",
    border: "1px solid transparent",
    background: "transparent",
    cursor: "pointer",
  },
  itemActive: { color: "var(--ink-0)", background: "var(--bg-2)", borderColor: "var(--line-hi)" },
  itemDisabled: { opacity: 0.55, cursor: "not-allowed" },
  group: {
    gridRow: "1 / span 2",
    color: "var(--ink-3)",
    fontFamily: "var(--font-mono)",
    fontSize: 10,
    textTransform: "uppercase",
  },
  label: { ...clippedText, color: "inherit" },
  detail: { ...clippedText, color: "var(--ink-3)", fontSize: 12 },
  empty: {
    width: "100%",
    minHeight: 46,
    display: "grid",
    gridTemplateColumns: "92px minmax(0, 1fr)",
    gap: "4px 10px",
    alignItems: "center",
    padding: "8px 10px",
    borderRadius: "var(--radius-sm)",
    color: "var(--ink-3)",
    textAlign: "left",
  },
} satisfies Record<string, CSSProperties>;
