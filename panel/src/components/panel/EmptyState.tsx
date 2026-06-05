import type { ReactNode } from "react";

interface EmptyStateAction {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  title?: string;
  variant?: "primary" | "ghost";
}

interface EmptyStateProps {
  title: string;
  children: ReactNode;
  icon?: ReactNode;
  command?: string;
  actions?: EmptyStateAction[];
}

export function EmptyState({ title, children, icon, command, actions = [] }: EmptyStateProps) {
  return (
    <div className="empty-state">
      {icon && <div className="empty-state-icon">{icon}</div>}
      <div className="empty-state-title">{title}</div>
      <div className="empty-state-copy">{children}</div>
      {command && <code className="empty-state-command">{command}</code>}
      {actions.length > 0 && (
        <div className="empty-state-actions">
          {actions.map((action) => (
            <button
              key={action.label}
              className={`btn ${action.variant === "ghost" ? "ghost" : "primary"}`}
              onClick={action.onClick}
              disabled={action.disabled}
              title={action.title}
            >
              {action.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
