import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { CopyPlan, CopyRequest } from "../lib/types";
import { defaultSelection, keepOffered } from "./CopyData";

/** How long the choices must stay still before the backend is asked. */
const SETTLE_MS = 400;

/** The parts of a copy the backend's plan depends on. The data ticks, the
 * ports and what happens to the source do not change what it finds. */
function planned(request: CopyRequest): CopyRequest {
  return { ...request, data_selection: [], port_overrides: {}, stop_source: true, keep_source_stopped: false };
}

/** The backend's plan for the copy the sheet describes, asked for a moment
 * after the choices settle. It answers for the request it was asked with:
 * while the choices differ from that, `planning` is true and there is no
 * plan, so an old plan never stands in for a new one. The data ticks start
 * from the plan's suggestions and keep the user's choices while only the
 * name or the folder changes. */
export function useCopyPlan(request: CopyRequest | null) {
  const planKey = request ? JSON.stringify(planned(request)) : null;
  const [answer, setAnswer] = useState<{ key: string; plan: CopyPlan | null; error: string | null } | null>(null);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  // Which source and destination the ticks were made for.
  const ticksFor = useRef<string | null>(null);

  useEffect(() => {
    if (planKey === null) return;
    const wanted = JSON.parse(planKey) as CopyRequest;
    const what = JSON.stringify({ source: wanted.source, project: wanted.project?.name, stack: wanted.stack_id, destination: wanted.destination });
    let current = true;
    const timer = window.setTimeout(() => {
      api.copyPlan(wanted).then(
        (found) => {
          if (!current) return;
          // Only another source or destination brings the defaults back: a
          // renamed destination must not re-tick data the user left out.
          const sameWhat = ticksFor.current === what;
          ticksFor.current = what;
          setTicked((previous) => (sameWhat ? keepOffered(previous, found.containers) : defaultSelection(found.containers)));
          setAnswer({ key: planKey, plan: found, error: null });
        },
        (err) => current && setAnswer({ key: planKey, plan: null, error: errorMessage(err) }),
      );
    }, SETTLE_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [planKey]);

  const answered = answer !== null && answer.key === planKey;
  return {
    plan: answered ? answer.plan : null,
    error: answered ? answer.error : null,
    planning: planKey !== null && !answered,
    ticked,
    setTicked,
  };
}
