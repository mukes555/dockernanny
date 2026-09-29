import { useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { HostRow, HostSnapshot } from "../lib/types";
import { useStore } from "../state/store";
import { SpinnerIcon } from "../ui/icons";
import { Button, Card, cx, Eyebrow, Toggle } from "../ui/primitives";
import { SetupChanges } from "./SetupChanges";

const ROW_DOT: Record<HostRow["state"], string> = {
  ok: "bg-good",
  missing: "bg-critical",
  restart: "bg-warning",
  unknown: "border border-ink-3",
};

const ROW_TEXT: Record<HostRow["state"], string> = {
  ok: "text-ink-2",
  missing: "text-critical",
  restart: "text-warning",
  unknown: "text-ink-2",
};

/** The sharing role's part of this computer's page: what this computer has,
 * a button that makes it ready, the pairing code for the other computer, and
 * a log. All the work is done by the engine; this only shows and asks. */
export function SharingSections() {
  const host = useStore((state) => state.host);
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const [error, setError] = useState<string | null>(null);

  const run = (action: () => Promise<void>) => {
    setError(null);
    void action().catch((err) => setError(errorMessage(err)));
  };

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-[15px] font-semibold text-ink">Let other computers run their stacks here</h2>
          <p className="mt-1 text-[13px] leading-relaxed text-ink-2">
            Two steps: make this computer ready, then pair it with the computer that will use it. After that, that computer runs Docker stacks here by itself.
          </p>
        </div>
        {settings?.share_this_computer ? (
          <Button
            size="sm"
            className="shrink-0"
            onClick={() => run(() => saveSettings({ ...settings, share_this_computer: false }))}
            title="Stop sharing: the pairing port closes and nothing else is touched. Paired computers keep their keys and can be let back in by turning it on again."
          >
            Turn sharing off
          </Button>
        ) : null}
      </div>
      {error ? <div className="text-[12px] text-critical">{error}</div> : null}

      {!settings?.share_this_computer ? (
        <Card title="Sharing is off">
          <div className="flex items-center justify-between gap-4 text-[13px] text-ink-2">
            <span>Turn it on to let other computers run their stacks here.</span>
            <Button tone="primary" onClick={() => settings && run(() => saveSettings({ ...settings, share_this_computer: true }))}>
              Share this computer
            </Button>
          </div>
        </Card>
      ) : null}

      {settings?.share_this_computer && !host?.probed ? (
        <div className="flex items-center gap-2 text-[13px] text-ink-2">
          <SpinnerIcon /> Looking at what this computer has…
        </div>
      ) : null}

      {settings?.share_this_computer && host?.probed ? <StatusSection host={host} onError={setError} /> : null}
      {settings?.share_this_computer && host?.probed ? <PairingSection host={host} onError={setError} /> : null}
      {settings?.share_this_computer && host?.probed ? <UsersSection host={host} /> : null}
      {settings?.share_this_computer ? <LogSection /> : null}
    </div>
  );
}

function StatusSection({ host, onError }: { host: HostSnapshot; onError: (message: string | null) => void }) {
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const os = useStore((state) => state.os);
  const [memory, setMemory] = useState(0);
  const [makePrivate, setMakePrivate] = useState(false);
  // Ticked by default because a sleeping computer cannot be reached, but shown before Set up runs.
  const [keepAwake, setKeepAwake] = useState(true);
  const isWindows = host.os.includes("Windows");
  const allOk = host.rows.every((row) => row.state !== "missing");
  const defaultMemory = host.total_memory_gb > 0 ? Math.max(2, Math.floor(host.total_memory_gb / 2)) : 8;
  const memoryGb = memory || settings?.host_memory_gb || defaultMemory;
  const publicNetwork = host.network?.public && host.network.name && !host.network.name.includes('"') ? host.network.name : null;

  // The slider and the Settings field are one value; it is saved when the thumb is let go.
  const saveMemory = () => {
    const unchanged = memory === 0 || memory === settings?.host_memory_gb;
    if (!settings || unchanged) return;
    void saveSettings({ ...settings, host_memory_gb: memory }).catch((err) => onError(errorMessage(err)));
  };

  const setup = () => {
    onError(null);
    api
      .hostSetup({ memory_gb: memoryGb, make_network_private: makePrivate ? publicNetwork : null, keep_awake: isWindows && keepAwake })
      .catch((err) => onError(errorMessage(err)));
  };

  return (
    <Card title="1. Set up this computer">
      <div className="space-y-1.5">
        {host.rows.map((row) => (
          <div key={row.name} className="flex items-center gap-3 text-[13px]">
            <span className={cx("h-2.5 w-2.5 shrink-0 rounded-full", ROW_DOT[row.state])} />
            <span className="w-32 shrink-0 font-medium text-ink">{row.name}</span>
            <span className={cx("min-w-0 truncate", ROW_TEXT[row.state])} title={row.detail}>
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
            onChange={(on) => void saveSettings({ ...settings, start_at_login: on }).catch((err) => onError(errorMessage(err)))}
            label="Start at login"
          />
        ) : null}
      </div>

      <p className="mt-3 text-[12px] text-ink-3">
        {isWindows
          ? "Windows asks once for administrator permission. The app is not signed, so it says Unknown publisher; that is expected."
          : host.os === "macOS"
            ? "macOS asks for your password once, to turn Remote Login on."
            : "Anything that needs root is one script, shown here or run through your password prompt."}
      </p>
    </Card>
  );
}

function PairingSection({ host, onError }: { host: HostSnapshot; onError: (message: string | null) => void }) {
  const pairing = host.pairing;
  const call = (action: () => Promise<void>) => {
    onError(null);
    void action().catch((err) => onError(errorMessage(err)));
  };
  const minutes = `${Math.floor(pairing.remaining_s / 60)}:${String(pairing.remaining_s % 60).padStart(2, "0")}`;

  if (!host.ready_for_pairing) {
    return (
      <Card title="2. Pair with another computer">
        <p className="text-[13px] text-ink-2">Finish step 1 first. Pairing becomes available once this computer is ready.</p>
      </Card>
    );
  }

  return (
    <Card title="2. Pair with another computer">
      {pairing.armed && pairing.code ? (
        <>
          <div className="flex flex-wrap gap-10">
            <div>
              <Eyebrow>Code</Eyebrow>
              <div className="mono mt-1 text-5xl font-semibold tracking-wider text-accent">{pairing.code}</div>
            </div>
            <div>
              <Eyebrow>This computer's address</Eyebrow>
              {host.addresses.map((address) => (
                <div key={address} className="mono mt-1 text-2xl font-semibold text-ink">
                  {address}
                </div>
              ))}
              {host.addresses.length === 0 ? <div className="mt-1 text-[13px] text-critical">no network address found</div> : null}
              <div className="mt-1 text-[12px] text-ink-3">
                User: {host.user ?? "?"} · SSH port: {host.ssh_port}
              </div>
            </div>
          </div>
          <p className="mt-4 text-[13px] text-ink-2">
            On the other computer open dockerNanny, click Add machine, and type this address and code in the top section. That is all.
          </p>
          <div className="mt-2 flex items-center gap-3 text-[12px] text-ink-3">
            <span>Pairing is on for another {minutes}.</span>
            <Button size="sm" onClick={() => call(api.hostDisarmPairing)}>
              Turn pairing off
            </Button>
          </div>
        </>
      ) : (
        <>
          <p className={cx("text-[13px]", pairing.locked ? "text-warning" : "text-ink-2")}>
            {pairing.locked
              ? "Pairing is locked after too many wrong codes."
              : "Pairing is off. Turn it on when you are at the other computer and ready to type the code."}
          </p>
          <div className="mt-3">
            <Button tone="primary" onClick={() => call(api.hostArmPairing)}>
              Turn pairing on for 10 minutes
            </Button>
          </div>
        </>
      )}
      {pairing.note ? <div className="mt-3 text-[13px] font-medium text-good">{pairing.note}</div> : null}
    </Card>
  );
}

/** Who uses this computer: everyone that paired, and whoever has an ssh
 * session open right now (a running stack, a copy, a terminal). */
function UsersSection({ host }: { host: HostSnapshot }) {
  const nameOf = (computer: { name: string | null; address: string }) => (computer.name ? computer.name : computer.address);
  return (
    <Card title="Computers using this one">
      <div className="text-[12px] font-semibold text-ink-2">Connected now</div>
      {host.connected.length === 0 ? <p className="mt-1 text-[13px] text-ink-2">No computer is connected right now.</p> : null}
      <div className="mt-1 space-y-1">
        {host.connected.map((computer) => (
          <div key={computer.address} className="flex items-center gap-3 text-[13px]">
            <span className="h-2 w-2 shrink-0 rounded-full bg-good pulse" />
            <span className="font-medium text-ink">{nameOf(computer)}</span>
            {computer.name ? <span className="mono text-ink-3">{computer.address}</span> : null}
            <span className="ml-auto tabular text-ink-3">since {clock(computer.since_ms)}</span>
          </div>
        ))}
      </div>
      <div className="mt-4 text-[12px] font-semibold text-ink-2">Paired</div>
      {host.paired.length === 0 ? (
        <p className="mt-1 text-[13px] text-ink-2">
          {host.ready_for_pairing
            ? "No computer has paired yet. Turn pairing on above and type the code on the other computer."
            : "No computer has paired yet. Pairing opens once step 1 is done."}
        </p>
      ) : null}
      <div className="mt-1 space-y-1">
        {host.paired.map((computer) => (
          <div key={computer.address} className="flex items-center gap-3 text-[13px]">
            <span className="font-medium text-ink">{computer.name || "another computer"}</span>
            <span className="mono text-ink-3">{computer.address}</span>
            <span className="ml-auto text-ink-3">
              {computer.key_type} · paired {new Date(computer.paired_at_ms).toLocaleDateString()}
            </span>
          </div>
        ))}
      </div>
    </Card>
  );
}

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function LogSection() {
  const log = useStore((state) => state.hostLog);
  const [open, setOpen] = useState(false);
  return (
    <Card title="Log" actions={<Toggle checked={open} onChange={setOpen} label="Show" />}>
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
