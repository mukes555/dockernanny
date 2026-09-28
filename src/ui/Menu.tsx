import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";

import { MoreIcon } from "./icons";
import { cx } from "./primitives";

const CloseMenu = createContext<() => void>(() => {});

/** A "…" button with a short list of actions under it. It opens on click and
 * closes on Escape, a click outside, or a chosen item; the keyboard moves
 * through the items with the arrow keys and lands back on the button. */
export function Menu({ label, onClose, className, width = "w-44", children }: { label: string; onClose?: () => void; className?: string; width?: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  // Kept in a ref so the listeners below are set up once per opening.
  const closed = useRef(onClose);
  useEffect(() => {
    closed.current = onClose;
  });

  const close = (refocus: boolean) => {
    setOpen(false);
    closed.current?.();
    if (refocus) button.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const items = () => Array.from(root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []);
    items()[0]?.focus();
    const onPointer = (event: PointerEvent) => {
      const outside = !root.current?.contains(event.target as Node);
      if (outside) close(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close(true);
        return;
      }
      // Tab moves on to the next control; a menu left open behind it would linger.
      if (event.key === "Tab") {
        close(false);
        return;
      }
      const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
      if (step === 0) return;
      event.preventDefault();
      const list = items();
      const at = list.indexOf(document.activeElement as HTMLElement);
      list[(at + step + list.length) % list.length]?.focus();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return (
    // The wrapper positions the list; a caller may place it absolutely instead of relatively.
    <div ref={root} className={className ?? "relative"}>
      <button
        ref={button}
        type="button"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        className={cx("rounded-md p-1 text-ink-3 transition hover:bg-surface-2 hover:text-ink", open && "bg-surface-2 text-ink")}
        onClick={() => (open ? close(false) : setOpen(true))}
      >
        <MoreIcon />
      </button>
      {open ? (
        <div role="menu" aria-label={label} className={cx("absolute top-full right-0 z-20 mt-1 overflow-hidden rounded-lg border border-line bg-surface-2 py-0.5 shadow-xl", width)}>
          <CloseMenu.Provider value={() => close(true)}>{children}</CloseMenu.Provider>
        </div>
      ) : null}
    </div>
  );
}

/** A line between groups of items: everyday ones, rarer ones, then the one that removes. */
export function MenuSeparator() {
  return <div role="separator" className="my-0.5 border-t border-line" />;
}

/** One action in a `Menu`. `keepOpen` is for a first click that only asks
 * for a second one, like "Remove" turning into "Confirm remove". */
export function MenuItem({ onClick, children, danger = false, disabled = false, keepOpen = false }: { onClick: () => void; children: ReactNode; danger?: boolean; disabled?: boolean; keepOpen?: boolean }) {
  const close = useContext(CloseMenu);
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      className={cx("block w-full px-3 py-1.5 text-left text-[12px] transition hover:bg-surface focus-visible:bg-surface disabled:opacity-40", danger ? "text-critical" : "text-ink-2 hover:text-ink")}
      onClick={() => {
        onClick();
        if (!keepOpen) close();
      }}
    >
      {children}
    </button>
  );
}
