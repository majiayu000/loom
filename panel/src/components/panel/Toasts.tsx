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
    <div className="toast-layer" aria-live="polite" aria-relevant="additions removals">
      {toasts.map((toast) => (
        <div className="toast" data-tone={toast.tone} key={toast.id} role="status">
          <div className="toast-copy">
            <div className="toast-title">{toast.title}</div>
            {toast.detail && <div className="toast-detail">{toast.detail}</div>}
          </div>
          <button className="toast-dismiss" type="button" onClick={() => onDismiss(toast.id)} aria-label="Dismiss toast">
            x
          </button>
        </div>
      ))}
    </div>
  );
}
