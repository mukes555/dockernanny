import { clock } from "../lib/format";
import { api } from "../lib/ipc";
import type { AvailableUpdate, UpdateStatus } from "../lib/types";
import { availableUpdate, useStore } from "../state/store";
import { CheckIcon, RefreshIcon } from "../ui/icons";
import { Button, Card, cx, ErrorLine, Toggle } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import type { SectionProps } from "./SettingsSections";

const VERSION = __APP_VERSION__;

/** A check that answers at once would flash by unseen; this long, it is
 * clear that it ran and what it found. */
const SHORTEST_CHECK_MS = 900;

const pause = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, Math.max(0, ms)));

/** Everything about updates in one place: this version, what the last check
 * found, checking now, checking by itself, and a found version's notes with
 * the button that installs it. The sidebar and the tray lead here. */
export function UpdatesSection({ settings, commit }: SectionProps) {
  const update = useStore(availableUpdate);
  const status = useStore((state) => state.updateStatus);
  const setUpdateStatus = useStore((state) => state.setUpdateStatus);
  const asking = useAction("inline");
  // The app also checks by itself every hour; that check shows here the same way.
  const checking = asking.busy || (status?.checking ?? false);

  const checkNow = () =>
    asking.run(async () => {
      const started = Date.now();
      try {
        setUpdateStatus(await api.checkForUpdate());
      } finally {
        await pause(SHORTEST_CHECK_MS - (Date.now() - started));
      }
    }, "Couldn't check");

  return (
    <>
      <Card>
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="text-[15px] font-semibold text-ink">dockerNanny {VERSION}</div>
            <StatusLine checking={checking} error={asking.error} status={status} update={update} automatic={settings.check_updates} />
          </div>
          <Button onClick={() => void checkNow()} busy={checking} className="shrink-0">
            <RefreshIcon /> Check now
          </Button>
        </div>
        <div className="mt-4 border-t border-line pt-4">
          <Toggle checked={settings.check_updates} onChange={(on) => commit({ check_updates: on })} label="Check automatically at start and every hour" />
          <p className="mt-1.5 text-[12px] text-ink-3">
            Only the version file of the latest GitHub release is fetched; nothing is installed until you choose to.
          </p>
        </div>
      </Card>
      {update ? <UpdateReady update={update} /> : null}
    </>
  );
}

interface Answer {
  text: string;
  tone: string;
  upToDate?: boolean;
}

interface StatusFacts {
  checking: boolean;
  error: string | null;
  status: UpdateStatus | null;
  update: AvailableUpdate | null;
  automatic: boolean;
}

/** One line that always takes the same room, so the card never jumps between answers. */
function StatusLine(facts: StatusFacts) {
  const answer = answerOf(facts);
  return (
    <div className={cx("mt-0.5 flex min-h-[18px] items-center gap-1 text-[12px]", answer.tone)} aria-live="polite">
      {answer.upToDate ? <CheckIcon size={12} className="shrink-0" /> : null}
      <span className="truncate" title={answer.text}>
        {answer.text}
      </span>
    </div>
  );
}

/** The most useful thing to say, in this order: working, failed now, found, the last answer. */
function answerOf({ checking, error, status, update, automatic }: StatusFacts): Answer {
  if (checking) return { text: "Checking GitHub for a new version…", tone: "text-ink-3" };
  if (error) return { text: error, tone: "text-critical" };
  if (update) return { text: `Version ${update.version} is available`, tone: "text-accent" };
  const checkedAt = status?.checked_ms ? clock(status.checked_ms) : null;
  if (!checkedAt || !status?.outcome) return { text: automatic ? "Not checked yet" : "Automatic checks are off", tone: "text-ink-3" };
  if (status.outcome === "failed") return { text: `The check at ${checkedAt} did not work: ${status.error ?? "no reason given"}`, tone: "text-warning" };
  return { text: `You have the latest version · checked ${checkedAt}`, tone: "text-good", upToDate: true };
}

/** What the progress line says while an update installs. */
function installWord(percent: number | null): string {
  if (percent === null) return "Downloading…";
  if (percent < 100) return `Downloading, ${percent}%`;
  return "Installing…";
}

/** A newer release, what is in it, and what installing does to the running
 * app. Nothing is downloaded until the user says so, because installing
 * restarts the app and its bridges. */
function UpdateReady({ update }: { update: AvailableUpdate }) {
  const os = useStore((state) => state.os);
  const progress = useStore((state) => state.updateProgress);
  const setProgress = useStore((state) => state.setUpdateProgress);
  const installing = useAction("inline");
  // Progress keeps arriving if the page was left and opened again mid-download.
  const working = installing.busy || progress !== null;
  const percent = progress === null ? null : Math.round(progress * 100);

  const install = async () => {
    setProgress(null);
    // The app restarts at the end, so success has nothing left to show.
    const installed = await installing.run(api.installUpdate);
    // A download that failed halfway leaves its last share behind, which
    // would keep the button spinning next to the error.
    if (!installed) setProgress(null);
  };

  return (
    <Card title={`What's new in ${update.version}`}>
      {update.notes ? (
        <pre className="selectable max-h-64 overflow-auto whitespace-pre-wrap rounded-lg border border-line bg-plane/60 px-3 py-2 font-sans text-[12px] leading-relaxed text-ink-2">
          {update.notes}
        </pre>
      ) : null}
      <p className="mt-3 text-[12px] leading-relaxed text-ink-2">
        Installing restarts dockerNanny. Stacks keep running on their machines; their ports on localhost come back a few seconds after the restart, and sharing
        pauses for the same moment.
        {os === "windows" ? " Windows shows a small progress window while it installs." : ""}
      </p>
      {working ? (
        <div className="mt-3">
          <div className="text-[12px] text-ink-2">{installWord(percent)}</div>
          <div
            role="progressbar"
            aria-label="Update download"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent ?? undefined}
            className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-hairline"
          >
            <div className="h-full rounded-full bg-accent transition-all" style={{ width: `${percent ?? 15}%` }} />
          </div>
        </div>
      ) : null}
      <ErrorLine error={installing.error} className="mt-3" />
      <div className="mt-4">
        <Button tone="primary" onClick={() => void install()} busy={working}>
          Restart and update
        </Button>
      </div>
    </Card>
  );
}
