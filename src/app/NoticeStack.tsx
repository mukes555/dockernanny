import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import type { Notice } from "../state/store";
import { useStore } from "../state/store";
import { XIcon } from "../ui/icons";
import { cx } from "../ui/primitives";

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
      <AnimatePresence>
        {notices.map((notice) => (
          <NoticeRow key={notice.id} notice={notice} />
        ))}
      </AnimatePresence>
    </div>
  );
}

function NoticeRow({ notice }: { notice: Notice }) {
  const dismiss = useStore((state) => state.dismissNotice);
  // Held while the pointer or the keyboard is on it, so it never vanishes mid-read.
  const [held, setHeld] = useState(false);
  useEffect(() => {
    if (held) return;
    const timer = window.setTimeout(() => dismiss(notice.id), notice.tone === "error" ? 9000 : 5000);
    return () => window.clearTimeout(timer);
  }, [notice.id, notice.tone, dismiss, held]);

  return (
    <motion.div
      onMouseEnter={() => setHeld(true)}
      onMouseLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)}
      onBlur={() => setHeld(false)}
      layout
      initial={{ y: 12, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 12, opacity: 0 }}
      transition={{ type: "spring", stiffness: 400, damping: 32 }}
      className={cx(
        "pointer-events-auto flex items-start gap-3 rounded-xl border bg-surface px-3.5 py-2.5 shadow-xl",
        notice.tone === "error" ? "border-critical/40" : "border-line",
      )}
    >
      <span className={cx("mt-1.5 h-2 w-2 shrink-0 rounded-full", notice.tone === "error" ? "bg-critical" : "bg-accent")} />
      <p className="selectable min-w-0 flex-1 text-[12px] leading-relaxed text-ink-2">{notice.text}</p>
      <button type="button" onClick={() => dismiss(notice.id)} className="shrink-0 rounded-md p-0.5 text-ink-3 hover:text-ink" aria-label="Dismiss">
        <XIcon size={12} />
      </button>
    </motion.div>
  );
}
