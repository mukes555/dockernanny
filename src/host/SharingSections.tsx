import { useState } from "react";

import { api } from "../lib/ipc";
import type { HostOs, HostRow, HostSnapshot } from "../lib/types";
import { useStore } from "../state/store";
import type { DotState } from "../ui/Badges";
import { StatusDot } from "../ui/Badges";
import { SpinnerIcon } from "../ui/icons";
import { Button, Card, cx, ErrorLine, Toggle } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import { PairingSection, UsersSection } from "./PairingSections";
import { SetupChanges } from "./SetupChanges";

/** A row's dot and text: something Set up will do is amber, like everywhere else. */
const ROW_LOOK: Record<HostRow["state"], { dot: DotState; text: string }> = {
  ok: { dot: "good", text: "text-ink-2" },
  missing: { dot: "attention", text: "text-warning" },
  restart: { dot: "attention", text: "text-warning" },
  unknown: { dot: "pending", text: "text-ink-2" },
};

/** What each OS asks for when Set up runs. */
const PERMISSION: Record<HostOs, string> = {
  windows: "Windows asks once for administrator permission. The app is not signed, so it says Unknown publisher; that is expected.",
  macos: "macOS asks for your password once, to turn Remote Login on.",
  linux: "Anything that needs root is one script, shown here or run through your password prompt.",
};

/** The sharing role's part of this computer's page: what this computer has,
 * a button that makes it ready, the pairing code for the other computer, and
 * a log. All the work is done by the engine; this only shows and asks. */
export function SharingSections() {
  const host = useStore((state) => state.host);
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const switching = useAction("inline");
  const sharing = settings?.share_this_computer ?? false;
  const probed = sharing && Boolean(host?.probed);
  const setSharing = (on: boolean) => settings && void switching.run(() => saveSettings({ ...settings, share_this_computer: on }));

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-[15px] font-semibold text-ink">Let other computers run their stacks here</h2>
          <p className="mt-1 text-[13px] leading-relaxed text-ink-2">
            Two steps: make this computer ready, then pair it with the computer that will use it. After that, that computer runs Docker stacks here by itself.
          </p>
        </div>
        {sharing ? (
          <Button
            size="sm"
            className="shrink-0"
            onClick={() => setSharing(false)}
            busy={switching.busy}
            title="Stop sharing: the pairing port closes and nothing else is touched. Paired computers keep their keys and can be let back in by turning it on again."
          >
            Turn sharing off
          </Button>
        ) : null}
      </div>
      <ErrorLine error={switching.error} className="mt-0" />

      {!sharing ? (
        <Card title="Sharing is off">
          <div className="flex items-center justify-between gap-4 text-[13px] text-ink-2">
            <span>Turn it on to let other computers run their stacks here.</span>
            <Button tone="primary" onClick={() => setSharing(true)} busy={switching.busy}>
              Share this computer
            </Button>
          </div>
        </Card>
      ) : null}

      {sharing && !host?.probed ? (
        <div className="flex items-center gap-2 text-[13px] text-ink-2">
          <SpinnerIcon /> Looking at what this computer has…
        </div>
      ) : null}

      {probed && host ? <StatusSection host={host} /> : null}
      {probed && host ? <PairingSection host={host} /> : null}
      {probed && host ? <UsersSection host={host} /> : null}
      {sharing ? <LogSection /> : null}
    </div>
  );
}

function StatusSection({ host }: { host: HostSnapshot }) {
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const os = useStore((state) => state.os);
  const [memory, setMemory] = useState(0);
  const [makePrivate, setMakePrivate] = useState(false);
  // Ticked by default because a sleeping computer cannot be reached, but shown before Set up runs.
  const [keepAwake, setKeepAwake] = useState(true);
  // Set up, the memory slider and the start-at-login switch all report here.
  const action = useAction("inline");
  const isWindows = os === "windows";
  const allOk = host.rows.every((row) => row.state !== "missing");
  const defaultMemory = host.total_memory_gb > 0 ? Math.max(2, Math.floor(host.total_memory_gb / 2)) : 8;
  // Only a value the user chose is sent: 0 keeps the memory line already in .wslconfig.
  const chosenMemory = memory || settings?.host_memory_gb || 0;
  const memoryGb = chosenMemory || defaultMemory;
  const publicNetwork = host.network?.public && host.network.name && !host.network.name.includes('"') ? host.network.name : null;

  // The slider and the Settings field are one value; it is saved when the thumb is let go.
  const saveMemory = () => {
    const unchanged = memory === 0 || memory === settings?.host_memory_gb;
    if (!settings || unchanged) return;
    void action.run(() => saveSettings({ ...settings, host_memory_gb: memory }));
  };

  const setup = () =>
    void action.run(() =>
      api.hostSetup({ memory_gb: chosenMemory, make_network_private: makePrivate ? publicNetwork : null, keep_awake: isWindows && keepAwake }),
    );

  return (
    <Card title="1. Set up this computer">
      <div className="space-y-1.5">
        {host.rows.map((row) => (
          <div key={row.name} className="flex items-center gap-3 text-[13px]">
            <StatusDot state={ROW_LOOK[row.state].dot} label={row.detail} size="lg" />
            <span className="w-32 shrink-0 font-medium text-ink">{row.name}</span>
            <span className={cx("min-w-0 truncate", ROW_LOOK[row.state].text)} title={row.detail}>
              {row.detail}
            </span>
          </div>
        ))}
      </div>

      {host.notice ? (
        <div
          className={cx(
            "mt-4 whitespace-pre-wrap rounded-lg px-3 py-2 text-[13px] font-medium",
            host.notice.failed ? "bg-critical/10 text-critical" : "bg-warning/10 text-warning",
          )}
        >
          {host.notice.text}
        </div>
      ) : null}

      {isWindows && host.total_memory_gb > 0 ? (
        <label className="mt-4 flex items-center gap-3 text-[13px] text-ink-2">
          <span>
            Docker may use up to {memoryGb} of this computer's {host.total_memory_gb} GB
          </span>
          <input
            type="range"
            min={2}
            max={Math.max(2, host.total_memory_gb - 2)}
            value={memoryGb}
            onChange={(e) => setMemory(Number(e.target.value))}
            onPointerUp={saveMemory}
            onKeyUp={saveMemory}
            onBlur={saveMemory}
            className="accent-accent"
          />
        </label>
      ) : null}

      <SetupChanges
        os={os}
        open={!allOk}
        keepAwake={keepAwake}
        onKeepAwake={setKeepAwake}
        publicNetwork={publicNetwork}
        makePrivate={makePrivate}
        onMakePrivate={setMakePrivate}
      />

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Button tone="primary" onClick={setup} busy={host.setup_running}>
          {allOk ? "Run set up again" : "Set up this computer"}
        </Button>
        {host.setup_running ? <span className="text-[13px] text-ink-2">Working. This can take a few minutes.</span> : null}
        {settings ? (
          <Toggle
            checked={settings.start_at_login}
            onChange={(on) => void action.run(() => saveSettings({ ...settings, start_at_login: on }))}
            label="Start at login"
          />
        ) : null}
      </div>
      <ErrorLine error={action.error} className="mt-3" />

      <p className="mt-3 text-[12px] text-ink-3">{PERMISSION[os]}</p>
    </Card>
  );
}

function LogSection() {
  const log = useStore((state) => state.hostLog);
  const [open, setOpen] = useState(false);
  return (
    <Card title="Log" actions={<Toggle checked={open} onChange={setOpen} label="Show the log" />}>
      {open ? (
        <div className="mono selectable max-h-72 overflow-auto rounded-lg bg-plane/60 px-3 py-2 text-[12px] leading-relaxed text-ink-2">
          {log.length === 0 ? <div className="text-ink-3">nothing yet</div> : log.map((line, index) => <div key={`${index}-${line}`}>{line}</div>)}
        </div>
      ) : (
        <p className="text-[12px] text-ink-3">What the sharing role did, also in host.log in the app folder.</p>
      )}
    </Card>
  );
}
