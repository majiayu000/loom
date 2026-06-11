import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
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
    <div className="palette-backdrop" role="presentation" onMouseDown={onClose}>
      <div
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="palette-input-row">
          <SearchIcon />
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
          />
          <kbd>Esc</kbd>
        </div>
        <div className="palette-results" role="listbox" aria-label="Command results">
          {filtered.length === 0 ? (
            <div className="palette-empty">No commands found.</div>
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
              >
                <span className="palette-group">{command.group}</span>
                <span className="palette-label">{command.label}</span>
                {command.detail && <span className="palette-detail">{command.detail}</span>}
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
