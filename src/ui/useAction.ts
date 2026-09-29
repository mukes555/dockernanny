import { useCallback, useState } from "react";

import { errorMessage } from "../lib/ipc";
import { useStore } from "../state/store";

/** Where a failure is told. "inline": next to the control, for forms, cards
 * and dialogs, which have room for a line (`<ErrorLine>`). "notice": a
 * notice at the bottom of the window, for menu items and page buttons that
 * have no room of their own. */
export type Report = "inline" | "notice";

/** One action's state: `run` it, `busy` while it works, `error` when it
 * failed (inline only). `run` says whether it went well, so the caller can
 * close a dialog or move on. A new run clears the last error. */
export function useAction(report: Report) {
  const pushNotice = useStore((state) => state.pushNotice);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(
    async (work: () => Promise<unknown>): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        await work();
        return true;
      } catch (err) {
        const message = errorMessage(err);
        if (report === "notice") pushNotice(message);
        else setError(message);
        return false;
      } finally {
        setBusy(false);
      }
    },
    [report, pushNotice],
  );

  return { run, busy, error, setError };
}
