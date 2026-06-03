import type { ReactNode } from "react";

interface ActionEmptyStateProps {
  title: string;
  body: ReactNode;
  primaryLabel: string;
  onPrimary?: () => void;
  primaryDisabled?: boolean;
  primaryTitle?: string;
  primaryIcon?: ReactNode;
  command?: string;
  compact?: boolean;
}

export function ActionEmptyState({
  title,
  body,
  primaryLabel,
  onPrimary,
  primaryDisabled = false,
  primaryTitle,
  primaryIcon,
  command,
  compact = false,
}: ActionEmptyStateProps) {
  return (
    <div className={`action-empty-state${compact ? " compact" : ""}`}>
      <div className="action-empty-state-title">{title}</div>
      <div className="action-empty-state-body">{body}</div>
      <div className="action-empty-state-actions">
        <button
          type="button"
          className="btn primary"
          onClick={onPrimary}
          disabled={primaryDisabled}
          title={primaryTitle}
        >
          {primaryIcon}
          {primaryLabel}
        </button>
      </div>
      {command && <code className="action-empty-state-command mono">{command}</code>}
    </div>
  );
}
