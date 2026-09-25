import { useId, useState, type ReactNode } from "react";

/** The words this app uses that a newcomer may not know, in plain language.
 * Help lists them all; `Term` shows one where the word first appears. */
export const GLOSSARY = {
  stack: {
    word: "Stack",
    text: "A Docker Compose project: the folder with a compose file, and the containers it starts.",
  },
  machine: {
    word: "Machine",
    text: "Another computer that runs stacks for this one. It needs ssh and Docker; dockerNanny reaches it over ssh.",
  },
  thisComputer: {
    word: "This computer",
    text: "The computer this window runs on. Your editor and browser stay here and use the stacks on localhost.",
  },
  bridge: {
    word: "Bridge",
    text: "The ssh tunnels that carry a stack's published ports from a machine to localhost here. Turn it off and the stack keeps running there, only unreachable from here.",
  },
  pairing: {
    word: "Pairing",
    text: "A one-time code shown on a shared computer and typed here. It installs this computer's ssh key there, so no password is ever typed.",
  },
  hostKey: {
    word: "Pinned host key",
    text: "The machine's ssh identity, saved during pairing. If a different computer ever answers at that address, ssh refuses to connect.",
  },
  dockerContext: {
    word: "Docker context",
    text: "A named target for the docker command. With the context on, `docker --context dn-<name> ps` in any terminal talks to that machine's Docker.",
  },
  sharing: {
    word: "Sharing",
    text: "The role that lets other computers on your network run their stacks on this one, after pairing.",
  },
  wsl: {
    word: "WSL",
    text: "Windows Subsystem for Linux. On Windows, Docker, sshd and the other Linux tools dockerNanny needs run inside it.",
  },
} satisfies Record<string, { word: string; text: string }>;

export type GlossaryKey = keyof typeof GLOSSARY;

/** A word with its definition on hover or keyboard focus. The dotted
 * underline says there is more to read; the definition is announced too. */
export function Term({ name, children }: { name: GlossaryKey; children?: ReactNode }) {
  const [shown, setShown] = useState(false);
  const tipId = useId();
  const entry = GLOSSARY[name];
  return (
    <span className="relative inline-block">
      <span
        tabIndex={0}
        aria-describedby={tipId}
        className="cursor-help rounded-sm underline decoration-ink-3 decoration-dotted underline-offset-[3px] outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        onMouseEnter={() => setShown(true)}
        onMouseLeave={() => setShown(false)}
        onFocus={() => setShown(true)}
        onBlur={() => setShown(false)}
        onKeyDown={(event) => {
          if (event.key === "Escape") setShown(false);
        }}
      >
        {children ?? entry.word}
      </span>
      {/* Rendered only while shown: a hidden box near an edge would still make the page scroll sideways. */}
      {shown ? (
        <span
          id={tipId}
          role="tooltip"
          className="pointer-events-none absolute top-full left-0 z-50 mt-1.5 w-64 rounded-lg border border-line bg-surface px-3 py-2 text-left text-[12px] leading-relaxed font-normal normal-case tracking-normal text-ink-2 shadow-xl"
        >
          {entry.text}
        </span>
      ) : null}
    </span>
  );
}
