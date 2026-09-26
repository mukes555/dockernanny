import { errorMessage } from "../lib/ipc";
import type { HostSnapshot } from "../lib/types";
import { useStore } from "../state/store";
import { OsGlyph } from "../ui/Badges";
import { Chip, cx } from "../ui/primitives";
import type { ChipTone } from "../ui/primitives";

/** The band at the top of the rail: where the app runs, who is logged in,
 * and which roles it plays. Deliberately not a card, so it never reads as
 * one more machine. Clicking it opens this computer's page. */
export function ThisComputerBlock() {
  const settings = useStore((state) => state.settings);
  const host = useStore((state) => state.host);
  const computerName = useStore((state) => state.computerName);
  const info = useStore((state) => state.computerInfo);
  const view = useStore((state) => state.view);
  const setView = useStore((state) => state.setView);
  const saveSettings = useStore((state) => state.saveSettings);
  const showSharing = useStore((state) => state.showSharing);
  const pushNotice = useStore((state) => state.pushNotice);

  const sharing = settings?.share_this_computer ?? false;
  const usingMachines = settings?.use_machines ?? true;
  const open = view === "computer";
  const sharingChip = sharingChipFor(sharing, host);
  const whoAndWhere = info ? `${info.user}@${info.probe.hostname ?? computerName}` : null;

  const turnSharingOn = () => {
    if (!settings) return;
    void saveSettings({ ...settings, share_this_computer: true })
      .then(showSharing)
      .catch((err) => pushNotice(`Sharing could not be turned on: ${errorMessage(err)}`));
  };

  return (
    <div className={cx("-mx-3 -mt-3 mb-3 border-b border-line px-4 pt-4 pb-3 transition", open ? "bg-accent-soft/60" : "bg-surface-2/50")}>
      <div className="text-[10px] uppercase tracking-[0.14em] text-ink-3">This computer</div>
      <button type="button" className="mt-1 flex w-full items-start gap-2.5 rounded-lg text-left" onClick={() => setView(open ? "stacks" : "computer")} title="Open this computer's page">
        <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-surface text-accent">
          <OsGlyph os={info?.probe.os} size={16} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[14px] font-semibold text-ink">{computerName || "this computer"}</span>
          {whoAndWhere ? (
            <span className="mono block truncate text-[11px] text-ink-3" title={whoAndWhere}>
              {whoAndWhere}
            </span>
          ) : null}
          {info?.probe.os ? (
            <span className="block truncate text-[11px] text-ink-3" title={info.probe.os}>
              {info.probe.os}
            </span>
          ) : null}
        </span>
      </button>
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        {usingMachines ? <Chip tone="neutral">using machines</Chip> : null}
        {sharing ? (
          <button type="button" onClick={showSharing} title="Open the sharing section">
            <Chip tone={sharingChip.tone}>{sharingChip.text}</Chip>
          </button>
        ) : (
          <button type="button" onClick={turnSharingOn} className="text-[11px] text-ink-3 underline-offset-2 hover:text-ink hover:underline" title="Let other computers run their stacks here">
            sharing off · turn on
          </button>
        )}
      </div>
    </div>
  );
}

/** The one thing most worth knowing about sharing; the first match wins. */
function sharingChipFor(sharing: boolean, host: HostSnapshot | null): { text: string; tone: ChipTone } {
  if (!sharing) return { text: "sharing off", tone: "neutral" };
  if (!host?.probed) return { text: "sharing: checking", tone: "neutral" };
  if (host.pairing.armed) return { text: `pairing on · ${host.pairing.code}`, tone: "accent" };
  const missing = host.rows.filter((row) => row.state === "missing").length;
  if (missing > 0) return { text: `sharing: ${missing} to set up`, tone: "warning" };
  if (host.rows.some((row) => row.state === "restart")) return { text: "sharing: restart once", tone: "warning" };
  if (host.connected.length > 0) return { text: `sharing: ${host.connected.length} connected`, tone: "good" };
  return { text: "sharing: ready", tone: "good" };
}
