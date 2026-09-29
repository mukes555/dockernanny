import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { MoreIcon } from "./icons";
import { cx } from "./primitives";

const CloseMenu = createContext<() => void>(() => {});

/** Below this much room under the button, the list opens upwards. */
const ROOM_FOR_LIST = 280;

/** How far an arrow key moves the focus through the items. */
const ARROW_STEP: Record<string, number | undefined> = { ArrowDown: 1, ArrowUp: -1 };

/** A "…" button with a short list of actions under it. It opens on click and
 * closes on Escape, a click outside, a scroll, or a chosen item; the keyboard
 * moves through the items with the arrow keys and lands back on the button.
 * The list is drawn on top of the whole window, next to the button, so no
 * scrolling sidebar or rounded table around the button can cut it off. */
export function Menu({
  label,
  onClose,
  className,
  width = "w-44",
  icon,
  children,
}: {
  label: string;
  onClose?: () => void;
  className?: string;
  width?: string;
  icon?: ReactNode;
  children: ReactNode;
}) {
  const [place, setPlace] = useState<CSSProperties | null>(null);
  const open = place !== null;
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  // Kept in a ref so the listeners below are set up once per opening.
  const closed = useRef(onClose);
  useEffect(() => {
    closed.current = onClose;
  });

  const openList = () => {
    const rect = button.current?.getBoundingClientRect();
    if (!rect) return;
    const right = window.innerWidth - rect.right;
    const roomBelow = window.innerHeight - rect.bottom;
    setPlace(roomBelow < ROOM_FOR_LIST ? { bottom: window.innerHeight - rect.top + 4, right } : { top: rect.bottom + 4, right });
  };
  const close = (refocus: boolean) => {
    setPlace(null);
    closed.current?.();
    if (refocus) button.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const items = () => Array.from(list.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []);
    items()[0]?.focus();
    const onPointer = (event: PointerEvent) => {
      const target = event.target as Node;
      const outside = !root.current?.contains(target) && !list.current?.contains(target);
      if (outside) close(false);
    };
    // The list stays where it was drawn, so a scroll that moves the button
    // closes it. Only that one: a log drawer following its output scrolls
    // several times a second and must not close a menu elsewhere.
    const onScroll = (event: Event) => {
      const scrolled = event.target;
      const movesTheButton = scrolled === document || (scrolled instanceof Node && scrolled.contains(button.current));
      if (movesTheButton) close(false);
    };
    const onResize = () => close(false);
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
      const step = ARROW_STEP[event.key];
      if (step === undefined) return;
      event.preventDefault();
      const all = items();
      const at = all.indexOf(document.activeElement as HTMLElement);
      all[(at + step + all.length) % all.length]?.focus();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  }, [open]);

  return (
    // The wrapper places the button; a caller may place it absolutely instead of relatively.
    <div ref={root} className={className ?? "relative"}>
      <button
        ref={button}
        type="button"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        className={cx("rounded-md p-1 text-ink-3 transition hover:bg-surface-2 hover:text-ink", open && "bg-surface-2 text-ink")}
        onClick={() => (open ? close(false) : openList())}
      >
        {icon ?? <MoreIcon />}
      </button>
      {place
        ? createPortal(
            <div
              ref={list}
              role="menu"
              aria-label={label}
              style={place}
              className={cx("fixed z-50 overflow-hidden rounded-lg border border-line bg-surface-2 py-0.5 shadow-xl", width)}
            >
              <CloseMenu.Provider value={() => close(true)}>{children}</CloseMenu.Provider>
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}

/** A line between groups of items: everyday ones, rarer ones, then the one that removes. */
export function MenuSeparator() {
  return <div role="separator" className="my-0.5 border-t border-line" />;
}

/** One action in a `Menu`. `keepOpen` is for a first click that only asks
 * for a second one, like "Remove" turning into "Confirm remove". */
export function MenuItem({
  onClick,
  children,
  danger = false,
  disabled = false,
  keepOpen = false,
}: {
  onClick: () => void;
  children: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  keepOpen?: boolean;
}) {
  const close = useContext(CloseMenu);
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      className={cx(
        "block w-full px-3 py-1.5 text-left text-[12px] transition hover:bg-surface focus-visible:bg-surface disabled:opacity-40",
        danger ? "text-critical" : "text-ink-2 hover:text-ink",
      )}
      onClick={() => {
        onClick();
        if (!keepOpen) close();
      }}
    >
      {children}
    </button>
  );
}
