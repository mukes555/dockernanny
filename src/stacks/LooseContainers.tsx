/** How many names are spelled out before "and N more". */
const NAMES_SHOWN = 4;

/** Names the containers this computer's Docker runs outside any compose
 * project, such as one started with `docker run`. A copy carries compose
 * projects only, so they are named here instead of being left out of the
 * list without a word. Nothing shows when there are none. */
export function LooseContainers({ names }: { names: string[] }) {
  if (names.length === 0) return null;

  const shown = names.slice(0, NAMES_SHOWN);
  const hidden = names.length - shown.length;
  const listed = hidden > 0 ? `${shown.join(", ")} and ${hidden} more` : shown.join(", ");
  const subject = names.length === 1 ? "It is" : "They are";

  return (
    <p className="text-[12px] leading-relaxed text-ink-3">
      Not listed: <span className="mono text-ink-2">{listed}</span>. {subject} not part of a Docker Compose project, and a copy carries compose projects only: a
      folder with a compose file, and its volumes.
    </p>
  );
}
