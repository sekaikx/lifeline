import { useEffect, useRef, type ReactNode } from "react";

/** Modal dialog on the native <dialog> element: focus trap, Esc to close and a backdrop for free. */
export function Dialog({
  open,
  title,
  onClose,
  children,
  footer,
  wide,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      className={`dialog${wide ? " wide" : ""}`}
      onClose={onClose}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      aria-label={title}
    >
      {open && (
        <>
          <header className="dialog-head">
            <h2>{title}</h2>
            <button className="row-btn" onClick={onClose} aria-label="Close">
              ✕
            </button>
          </header>
          <div className="dialog-body">{children}</div>
          {footer && <footer className="dialog-foot">{footer}</footer>}
        </>
      )}
    </dialog>
  );
}

/** Small coloured label: "local", "free", "cold chain"... */
export function Badge({ tone = "neutral", children, title }: { tone?: "neutral" | "accent" | "good" | "warn" | "critical"; children: ReactNode; title?: string }) {
  return (
    <span className={`tag ${tone}`} title={title}>
      {children}
    </span>
  );
}
