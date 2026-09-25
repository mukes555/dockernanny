import { useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import { useStore } from "../state/store";
import { BookIcon, ExternalIcon, FolderIcon, LogsIcon, SpinnerIcon } from "../ui/icons";
import { Button, Card, PageHeader } from "../ui/primitives";
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

/** Where a stranger goes when unsure: what the app is, where its files are,
 * what its words mean, and a report they can paste into an issue safely. */
export function HelpPage() {
  const setView = useStore((state) => state.setView);
  const setWelcomeOpen = useStore((state) => state.setWelcomeOpen);
  const os = useStore((state) => state.os);
  const [error, setError] = useState<string | null>(null);

  const open = (url: string) => void api.openLink(url).catch((err) => setError(errorMessage(err)));
  const reveal = (which: "folder" | "log") => void api.revealAppFile(which).catch((err) => setError(errorMessage(err)));

  return (
    <div className="mx-auto max-w-2xl space-y-4 pb-10">
      <PageHeader eyebrow="Help" title="Help and about" onBack={() => setView("stacks")} description={`dockerNanny ${VERSION} for ${OS_NAMES[os]}. Run Docker Compose stacks on other computers and use them on localhost here.`} />
      {error ? <div className="text-[12px] text-critical">{error}</div> : null}

      <Card title="Getting started">
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => setWelcomeOpen(true)}>Show the welcome again</Button>
          <Button onClick={() => setView("guide")}>
            <BookIcon /> Prepare another machine
          </Button>
        </div>
      </Card>

      <UpdatesCard />

      <Diagnostics onOpenIssue={() => open(`${REPO}/issues/new/choose`)} />

      <Card title="Files on this computer" description="Settings, machines, stacks and the log all live in one folder; nothing is kept anywhere else.">
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => reveal("folder")}>
            <FolderIcon /> Open the data folder
          </Button>
          <Button onClick={() => reveal("log")}>
            <LogsIcon /> Show the log file
          </Button>
        </div>
      </Card>

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
    </div>
  );
}

/** The check the automatic one does quietly, with its answer shown. */
function UpdatesCard() {
  const update = useStore((state) => state.update);
  const setUpdate = useStore((state) => state.setUpdate);
  const setUpdateOpen = useStore((state) => state.setUpdateOpen);
  const [checking, setChecking] = useState(false);
  const [answer, setAnswer] = useState<string | null>(null);

  const checkNow = async () => {
    setChecking(true);
    setAnswer(null);
    try {
      const found = await api.checkUpdate();
      setUpdate(found);
      setAnswer(found ? null : `dockerNanny ${VERSION} is the latest version.`);
    } catch (err) {
      setAnswer(`Could not check: ${errorMessage(err)}`);
    } finally {
      setChecking(false);
    }
  };

  return (
    <Card title="Updates" description={`This is dockerNanny ${VERSION}. New versions come from the project's GitHub releases.`}>
      <div className="flex flex-wrap items-center gap-3">
        {update ? (
          <Button tone="primary" onClick={() => setUpdateOpen(true)}>
            See what is new in {update.version}
          </Button>
        ) : (
          <Button onClick={() => void checkNow()} disabled={checking}>
            {checking ? <SpinnerIcon /> : null} Check for updates
          </Button>
        )}
        {answer ? <span className="text-[12px] text-ink-2">{answer}</span> : null}
      </div>
    </Card>
  );
}

/** Copies a redacted report and shows exactly what was copied, so nobody
 * pastes something into a public issue without seeing it first. */
export function Diagnostics({ onOpenIssue }: { onOpenIssue?: () => void }) {
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
    <Card title="When something goes wrong" description="Copy a report to paste into an issue. Names, addresses, users and paths are replaced with placeholders before it is copied.">
      <div className="flex flex-wrap gap-2">
        <Button tone="primary" onClick={() => void copy()} disabled={busy}>
          {busy ? <SpinnerIcon /> : null} Copy diagnostics
        </Button>
        {onOpenIssue ? (
          <Button onClick={onOpenIssue}>
            Report a problem <ExternalIcon size={11} />
          </Button>
        ) : null}
      </div>
      {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}
      {report ? (
        <div className="mt-3">
          <div className="text-[12px] text-good">Copied. This is exactly what is on the clipboard:</div>
          <pre className="mono selectable mt-2 max-h-64 overflow-auto rounded-lg border border-line bg-plane/60 px-3 py-2 text-[11px] leading-[1.6] text-ink-2">{report}</pre>
        </div>
      ) : null}
    </Card>
  );
}
