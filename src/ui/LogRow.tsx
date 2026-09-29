import { memo } from "react";

import type { LogEntry } from "../lib/types";
import { cx } from "./primitives";

/** One line of a log drawer. Memoized and keyed by `entry.seq`, so a new
 * batch renders only its own lines, not the 2000 already shown. */
export const LogRow = memo(function LogRow({ entry, label }: { entry: LogEntry; label?: string }) {
  return (
    <div className="flex gap-2 whitespace-pre-wrap break-all">
      {label ? <span className="shrink-0 text-accent">{label}</span> : null}
      <span className={cx(entry.stream === "stderr" ? "text-ink" : "text-ink-2")}>{entry.text}</span>
    </div>
  );
});
