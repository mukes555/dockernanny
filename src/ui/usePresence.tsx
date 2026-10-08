import { memo, useEffect, useState, type ReactNode } from "react";

/** How long a closing panel stays in the page so its exit can play. Every
 * exit transition in index.css ends within this. */
export const EXIT_MS = 160;

export type PresenceState = "open" | "closed";

/** Keeps a panel that comes and goes (a dialog, a drawer, the drop overlay)
 * in the page until its exit has played. `mounted` says whether to render it
 * at all; `state` goes on it as `data-state`, and the transitions in
 * index.css follow that attribute. Opening shows it at once. */
export function usePresence(open: boolean): { mounted: boolean; state: PresenceState } {
  const [mounted, setMounted] = useState(open);
  // Adjusted while rendering, React's way for state that follows a prop.
  if (open && !mounted) setMounted(true);

  useEffect(() => {
    if (open) return;
    const timer = window.setTimeout(() => setMounted(false), EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [open]);

  return { mounted: open || mounted, state: open ? "open" : "closed" };
}

interface HoldProps {
  closing: boolean;
  children: ReactNode;
}

/** While a panel plays its exit, it keeps showing what it showed when it was
 * last open. Callers clear their content as they close (the logs, the
 * title), and a panel that empties while it fades out looks broken. memo
 * skips a render when told the props did not change; saying so for as long
 * as `closing` holds keeps that last picture. */
export const HoldWhileClosing = memo(
  function HoldWhileClosing({ children }: HoldProps) {
    return <>{children}</>;
  },
  (_before: HoldProps, now: HoldProps) => now.closing,
);
