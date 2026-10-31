import { useEffect, useRef, type ReactNode } from "react";

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
}

export function Modal({ title, onClose, children }: ModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    dialogRef.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
      role="presentation"
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="glass-panel w-full max-w-lg p-5"
        onClick={(event) => {
          event.stopPropagation();
        }}
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="font-display text-base font-semibold">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close dialog"
            className="rounded-md px-2 py-1 text-slate-400 hover:bg-white/5 hover:text-slate-100"
          >
            ✕
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
