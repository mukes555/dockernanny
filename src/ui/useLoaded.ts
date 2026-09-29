import { useCallback, useEffect, useState } from "react";

import { errorMessage } from "../lib/ipc";

/** Something read from the backend when the component mounts, and again on
 * `reload`: the answer, why it failed, and whether a read is under way.
 * `load` must not change between renders (an `api` method does not). */
export function useLoaded<T>(load: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  // `isCurrent` is false once the component is gone, so a late answer is dropped.
  const read = useCallback(
    async (isCurrent: () => boolean) => {
      try {
        const answer = await load();
        if (!isCurrent()) return;
        setData(answer);
        setError(null);
      } catch (err) {
        if (isCurrent()) setError(errorMessage(err));
      } finally {
        if (isCurrent()) setLoading(false);
      }
    },
    [load],
  );

  useEffect(() => {
    let current = true;
    const first = window.setTimeout(() => void read(() => current), 0);
    return () => {
      current = false;
      window.clearTimeout(first);
    };
  }, [read]);

  const reload = () => {
    setLoading(true);
    return read(() => true);
  };

  return { data, error, loading, reload };
}
