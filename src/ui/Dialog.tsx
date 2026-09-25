import { useEffect, useId, useRef, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** The end of a long dialog: an error, if any, and the buttons, kept in view
 * while the dialog scrolls, so the main action never hides below the fold.
 * The negative margins reach the panel's padding so it sits flush at the bottom. */
export function DialogActions({ error, children }: { error?: string | null; children: ReactNode }) {
  return (
    <div className="sticky -bottom-6 -mx-6 -mb-6 mt-6 border-t border-line bg-surface px-6 py-4">
      {error ? <div className="mb-3 text-[12px] text-critical">{error}</div> : null}
      <div className="flex items-center justify-between gap-3">{children}</div>
    </div>
  );
}

/** A centred modal. The webview has no working window.confirm, so every
 * confirmation in the app goes through this or an in-button second click.
 * Keyboard users land inside it, Tab stays inside it, Escape closes it from
 * anywhere, and focus goes back where it was. Forms pass
 * `closeOnBackdrop={false}` so a stray click does not throw away what was typed. */
export function Dialog({
  open,
  onClose,
  title,
  eyebrow,
  width = 420,
  closeOnBackdrop = true,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  eyebrow?: ReactNode;
  width?: number;
  closeOnBackdrop?: boolean;
  children: ReactNode;
}) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const pressedBackdrop = useRef(false);
  // Callers pass a fresh arrow each render; keeping it in a ref stops the
  // focus handling below from re-running (and stealing focus) on every keystroke.
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    if (!open) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusFirst = window.setTimeout(() => {
      const first = panel.current?.querySelector<HTMLElement>(FOCUSABLE);
      (first ?? panel.current)?.focus();
    }, 0);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // Marked as handled so a panel under the dialog stays open (see useEscape).
        event.preventDefault();
        close.current();
        return;
      }
      if (event.key !== "Tab" || !panel.current) return;
      const items = Array.from(panel.current.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const wrapBack = event.shiftKey && document.activeElement === first;
      const wrapForward = !event.shiftKey && document.activeElement === last;
      if (wrapBack) {
        event.preventDefault();
        last.focus();
      } else if (wrapForward) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(focusFirst);
      document.removeEventListener("keydown", onKey);
      before?.focus();
    };
  }, [open]);

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          // Only a click that also started on the backdrop closes: selecting a
          // command and letting go outside the panel is not a request to close.
          onMouseDown={(event) => {
            pressedBackdrop.current = event.target === event.currentTarget;
          }}
          onClick={(event) => {
            const onBackdrop = event.target === event.currentTarget && pressedBackdrop.current;
            if (closeOnBackdrop && onBackdrop) onClose();
          }}
        >
          <motion.div
            ref={panel}
            role="dialog"
            aria-modal
            aria-labelledby={titleId}
            tabIndex={-1}
            className="max-h-[85vh] overflow-auto rounded-2xl border border-line bg-surface p-6 shadow-2xl outline-none"
            style={{ width }}
            initial={{ y: 14, scale: 0.97, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: 8, scale: 0.98, opacity: 0 }}
            transition={{ type: "spring", stiffness: 380, damping: 30 }}
            onClick={(event) => event.stopPropagation()}
          >
            {eyebrow ? <div className="text-[11px] uppercase tracking-[0.14em] text-accent">{eyebrow}</div> : null}
            <h2 id={titleId} className="mt-1 text-lg font-semibold text-ink">
              {title}
            </h2>
            {children}
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
