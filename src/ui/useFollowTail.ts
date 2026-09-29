import { useEffect, useRef } from "react";

/** Keeps a growing list scrolled to its end, the way `tail -f` does, unless
 * the reader scrolled up to look at something or `paused` is set. Pass the
 * list's length; attach `scroller` and `onScroll` to the scrolling element. */
export function useFollowTail(length: number, paused = false) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  useEffect(() => {
    const el = scroller.current;
    if (el && pinned.current && !paused) el.scrollTop = el.scrollHeight;
  }, [length, paused]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    // Within a line of the end counts as at the end.
    pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };

  return { scroller, onScroll };
}
