import { useEffect, useState } from "react";

/** The current time, fresh every `everyMs` while `active`, for text such as
 * "synced 12s ago". Inactive (a closed drawer), it does not tick at all. */
export function useNow(everyMs: number, active = true): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const tick = () => setNow(Date.now());
    // Once at once too: a drawer opened after a long pause must not show the old time for a second.
    const first = window.setTimeout(tick, 0);
    const timer = window.setInterval(tick, everyMs);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [everyMs, active]);
  return now;
}
