import type { CSSProperties } from "react";

export interface ToastViewModel {
  id: string;
  tone: "info" | "success" | "warn" | "error";
  title: string;
  detail?: string;
}

interface ToastsProps {
  toasts: ToastViewModel[];
  onDismiss: (id: string) => void;
}

export function Toasts({ toasts, onDismiss }: ToastsProps) {
  return (
    <div className="toast-layer" aria-live="polite" aria-relevant="additions removals" style={toastStyles.layer}>
      {toasts.map((toast) => (
        <div
          className="toast"
          data-tone={toast.tone}
          key={toast.id}
          role="status"
          style={{ ...toastStyles.toast, borderColor: toastBorderColor[toast.tone] }}
        >
          <div className="toast-copy">
            <div className="toast-title" style={toastStyles.title}>{toast.title}</div>
            {toast.detail && <div className="toast-detail" style={toastStyles.detail}>{toast.detail}</div>}
          </div>
          <button
            className="toast-dismiss"
            type="button"
            onClick={() => onDismiss(toast.id)}
            aria-label="Dismiss toast"
            style={toastStyles.dismiss}
          >
            x
          </button>
        </div>
      ))}
    </div>
  );
}

const toastBorderColor: Record<ToastViewModel["tone"], string> = {
  info: "var(--line-hi)",
  success: "color-mix(in srgb, var(--ok) 45%, var(--line-hi))",
  warn: "color-mix(in srgb, var(--warn) 50%, var(--line-hi))",
  error: "color-mix(in srgb, var(--err) 50%, var(--line-hi))",
};

const toastStyles = {
  layer: {
    position: "fixed",
    right: 16,
    bottom: 44,
    zIndex: 100,
    display: "grid",
    gap: 8,
    width: "min(360px, calc(100vw - 32px))",
  },
  toast: {
    display: "flex",
    alignItems: "flex-start",
    justifyContent: "space-between",
    gap: 12,
    padding: "10px 12px",
    border: "1px solid var(--line-hi)",
    borderRadius: "var(--radius)",
    background: "var(--bg-1)",
    boxShadow: "var(--shadow)",
  },
  title: { color: "var(--ink-0)", fontWeight: 600 },
  detail: { marginTop: 2, color: "var(--ink-2)", fontSize: 12, overflowWrap: "anywhere" },
  dismiss: { color: "var(--ink-3)", cursor: "pointer" },
} satisfies Record<string, CSSProperties>;
