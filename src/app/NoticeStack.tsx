import { useEffect, useState } from "react";

import type { Notice } from "../state/store";
import { useStore } from "../state/store";
import { XIcon } from "../ui/icons";
import { cx } from "../ui/primitives";
import { EXIT_MS } from "../ui/usePresence";

/** Failures the user should see but did not ask about: a background copy that
 * failed, a startup load that did not answer, an unexpected error. Shown in
 * the corner, dismissible, and gone on their own after a while. */
export function NoticeStack() {
  const notices = useStore((state) => state.notices);
  // Always in the page, even when empty: a live region only announces what is added to it.
  return (
    <div
      role="status"
      aria-live="polite"
      className="pointer-events-none fixed bottom-4 left-1/2 z-40 flex w-full max-w-md -translate-x-1/2 flex-col gap-2 px-4"
    >
      {notices.map((notice) => (
        <NoticeRow key={notice.id} notice={notice} />
      ))}
    </div>
  );
}

function NoticeRow({ notice }: { notice: Notice }) {
  const dismiss = useStore((state) => state.dismissNotice);
  // Held while the pointer or the keyboard is on it, so it never vanishes mid-read.
  const [held, setHeld] = useState(false);
  // A notice fades out first and leaves the store after: once gone from the
  // store, there is nothing left to fade.
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    if (held || leaving) return;
    const timer = window.setTimeout(() => setLeaving(true), notice.tone === "error" ? 9000 : 5000);
    return () => window.clearTimeout(timer);
  }, [notice.tone, held, leaving]);

  useEffect(() => {
    if (!leaving) return;
    const timer = window.setTimeout(() => dismiss(notice.id), EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [leaving, notice.id, dismiss]);

  return (
    <div
      onMouseEnter={() => setHeld(true)}
      onMouseLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
      data-state={leaving ? "closed" : "open"}
      className={cx(
        "notice-row pointer-events-auto flex items-start gap-3 rounded-xl border bg-surface px-3.5 py-2.5 shadow-xl",
        notice.tone === "error" ? "border-critical/40" : "border-line",
      )}
    >
      <span className={cx("mt-1.5 h-2 w-2 shrink-0 rounded-full", notice.tone === "error" ? "bg-critical" : "bg-accent")} />
      <p className="selectable min-w-0 flex-1 text-[12px] leading-relaxed text-ink-2">{notice.text}</p>
      <button type="button" onClick={() => setLeaving(true)} className="shrink-0 rounded-md p-0.5 text-ink-3 hover:text-ink" aria-label="Dismiss">
        <XIcon size={12} />
      </button>
    </div>
  );
}
