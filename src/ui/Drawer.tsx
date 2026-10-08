import { useEffect, useId, useRef, type ReactNode } from "react";

import { XIcon } from "./icons";
import { Button, Eyebrow } from "./primitives";
import { useEscape } from "./useEscape";
import { HoldWhileClosing, usePresence } from "./usePresence";

/** A panel along the right edge for something that keeps going while the
 * rest of the window stays usable: logs, a copy's progress. It is named for
 * screen readers by its title, takes the focus when it opens (so the keyboard
 * is inside it), closes on Escape, and gives the focus back to whatever
 * opened it. Not modal: the page behind it still works. */
export function Drawer({
  open,
  onClose,
  eyebrow,
  title,
  closeLabel,
  controls,
  footer,
  width = 560,
  children,
}: {
  open: boolean;
  onClose: () => void;
  /** The small line above the title: "Logs", "Copying". */
  eyebrow: ReactNode;
  title: ReactNode;
  /** What the close button says to a screen reader: "Close logs". */
  closeLabel: string;
  /** Buttons and pickers next to the title. */
  controls?: ReactNode;
  footer?: ReactNode;
  width?: number;
  children: ReactNode;
}) {
  const titleId = useId();
  const panel = useRef<HTMLElement>(null);
  const { mounted, state } = usePresence(open);
  useEscape(open, onClose);

  useEffect(() => {
    if (!open) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusIn = window.setTimeout(() => panel.current?.focus(), 0);
    return () => {
      window.clearTimeout(focusIn);
      before?.focus();
    };
  }, [open]);

  if (!mounted) return null;
  return (
    <aside
      ref={panel}
      role="dialog"
      aria-labelledby={titleId}
      tabIndex={-1}
      data-state={state}
      className="drawer-panel fixed top-0 bottom-0 right-0 z-20 flex max-w-[80vw] flex-col border-l border-line bg-surface shadow-2xl outline-none"
      style={{ width }}
    >
      <HoldWhileClosing closing={!open}>
        <header className="flex items-center gap-2 border-b border-line px-4 py-3">
          <div className="min-w-0 flex-1">
            <Eyebrow>{eyebrow}</Eyebrow>
            <h2 id={titleId} className="truncate text-[14px] font-semibold text-ink">
              {title}
            </h2>
          </div>
          {controls}
          <Button size="sm" tone="ghost" onClick={onClose} aria-label={closeLabel}>
            <XIcon />
          </Button>
        </header>
        {children}
        {footer ? <footer className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">{footer}</footer> : null}
      </HoldWhileClosing>
    </aside>
  );
}
