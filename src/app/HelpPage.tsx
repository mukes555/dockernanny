import { useState, type ReactNode } from "react";

import { api, errorMessage } from "../lib/ipc";
import { useStore } from "../state/store";
import { BookIcon, ExternalIcon, FolderIcon, LogsIcon } from "../ui/icons";
import { Page } from "../ui/Page";
import { Button, Card } from "../ui/primitives";
import { GLOSSARY } from "../ui/Term";

const VERSION = __APP_VERSION__;
const REPO = "https://github.com/mukes555/dockernanny";

const LINKS: Array<{ label: string; url: string }> = [
  { label: "Read me", url: `${REPO}#readme` },
  { label: "Report a problem", url: `${REPO}/issues/new/choose` },
  { label: "Releases", url: `${REPO}/releases` },
  { label: "License (MIT)", url: `${REPO}/blob/main/LICENSE` },
];

const OS_NAMES = { macos: "macOS", windows: "Windows", linux: "Linux" } as const;

/** Where a stranger goes when unsure: how to begin, what to do when
 * something breaks (a report they can paste into an issue safely, the log,
 * the data folder), and what the app's words mean. */
export function HelpPage() {
  const setView = useStore((state) => state.setView);
  const setWelcomeOpen = useStore((state) => state.setWelcomeOpen);
  const os = useStore((state) => state.os);
  const [error, setError] = useState<string | null>(null);

  const open = (url: string) => void api.openLink(url).catch((err) => setError(errorMessage(err)));
  const reveal = (which: "folder" | "log") => void api.revealAppFile(which).catch((err) => setError(errorMessage(err)));

  return (
    <Page
      title="Help"
      summary={`dockerNanny ${VERSION} for ${OS_NAMES[os]}: run Docker Compose stacks on other computers and use them on localhost here`}
      width="max-w-3xl"
    >
      {error ? <div className="text-[12px] text-critical">{error}</div> : null}

      <Card title="Getting started" description="The three-step welcome, and how to get a machine ready by hand.">
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => setWelcomeOpen(true)}>Show the welcome again</Button>
          <Button onClick={() => setView("guide")}>
            <BookIcon /> How to prepare a machine
          </Button>
        </div>
      </Card>

      <Diagnostics onOpenIssue={() => open(`${REPO}/issues/new/choose`)}>
        <Button onClick={() => reveal("log")}>
          <LogsIcon /> Show the log file
        </Button>
        <Button onClick={() => reveal("folder")} title="Settings, machines, stacks and the log all live in this one folder">
          <FolderIcon /> Open the data folder
        </Button>
      </Diagnostics>

      <Card title="Words used here">
        <dl className="space-y-3">
          {Object.values(GLOSSARY).map((entry) => (
            <div key={entry.word}>
              <dt className="text-[13px] font-medium text-ink">{entry.word}</dt>
              <dd className="mt-0.5 text-[13px] leading-relaxed text-ink-2">{entry.text}</dd>
            </div>
          ))}
        </dl>
      </Card>

      <Card title="Links">
        <div className="flex flex-wrap gap-2">
          {LINKS.map((link) => (
            <Button key={link.url} tone="ghost" onClick={() => open(link.url)}>
              {link.label} <ExternalIcon size={11} />
            </Button>
          ))}
        </div>
      </Card>
    </Page>
  );
}

/** Copies a redacted report and shows exactly what was copied, so nobody
 * pastes something into a public issue without seeing it first. The page
 * adds its own buttons after the two here. */
function Diagnostics({ onOpenIssue, children }: { onOpenIssue: () => void; children?: ReactNode }) {
  const [report, setReport] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const copy = async () => {
    setBusy(true);
    setError(null);
    try {
      const text = await api.diagnostics();
      await api.copyText(text);
      setReport(text);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card
      title="When something goes wrong"
      description="Copy a report to paste into an issue. Names, addresses, users and paths are replaced with placeholders before it is copied."
    >
      <div className="flex flex-wrap gap-2">
        <Button tone="primary" onClick={() => void copy()} busy={busy}>
          Copy diagnostics
        </Button>
        <Button onClick={onOpenIssue}>
          Report a problem <ExternalIcon size={11} />
        </Button>
        {children}
      </div>
      {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}
      {report ? (
        <div className="mt-3">
          <div className="text-[12px] text-good">Copied. This is exactly what is on the clipboard:</div>
          <pre className="mono selectable mt-2 max-h-64 overflow-auto rounded-lg border border-line bg-plane/60 px-3 py-2 text-[11px] leading-[1.6] text-ink-2">
            {report}
          </pre>
        </div>
      ) : null}
    </Card>
  );
}
