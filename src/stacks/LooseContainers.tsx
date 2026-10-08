import { api } from "../lib/ipc";
import { useLoaded } from "../ui/useLoaded";

/** How many names are spelled out before "and N more". */
const NAMES_SHOWN = 4;

/** Containers this computer's Docker runs outside any compose project, such
 * as one started with `docker run`. A copy carries compose projects only, so
 * they are named here instead of being left out of the list without a word.
 * Nothing shows while Docker is unreadable: the project list says why. */
export function LooseContainers() {
  const loose = useLoaded(api.looseContainers);
  const containers = loose.data ?? [];
  if (containers.length === 0) return null;

  const shown = containers.slice(0, NAMES_SHOWN).map((c) => c.name);
  const hidden = containers.length - shown.length;
  const names = hidden > 0 ? `${shown.join(", ")} and ${hidden} more` : shown.join(", ");
  const subject = containers.length === 1 ? "It is" : "They are";

  return (
    <p className="text-[12px] leading-relaxed text-ink-3">
      Not listed: <span className="mono text-ink-2">{names}</span>. {subject} not part of a Docker Compose project, and a copy carries compose projects only: a
      folder with a compose file, and its volumes.
    </p>
  );
}
