import type { Toast as ToastModel } from "../hooks/useToasts";

/** Stack of auto-expiring notifications, rendered top-right above the chrome. */
export function ToastStack({
  toasts,
  onDismiss,
}: {
  toasts: ToastModel[];
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-stack">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`} onMouseDown={(e) => e.stopPropagation()}>
          <span className="toast-msg">{t.message}</span>
          <button className="toast-close" onClick={() => onDismiss(t.id)} aria-label="Dismiss">
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}