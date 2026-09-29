import { useState } from "react";

import { clock } from "../lib/format";
import { api } from "../lib/ipc";
import type { HostSnapshot, PairedComputer } from "../lib/types";
import { StatusDot } from "../ui/Badges";
import { Button, Card, cx, ErrorLine, Eyebrow } from "../ui/primitives";
import { useAction } from "../ui/useAction";

/** Step 2 of sharing: the code and address another computer types, while pairing is on. */
export function PairingSection({ host }: { host: HostSnapshot }) {
  const pairing = host.pairing;
  const flip = useAction("inline");
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
          {host.host_fingerprint ? (
            <p className="mt-1 text-[12px] text-ink-3">
              After pairing, the other computer shows this computer's host key: <span className="mono selectable text-ink-2">{host.host_fingerprint}</span>. If
              it shows another, forget that computer below.
            </p>
          ) : null}
          <div className="mt-2 flex items-center gap-3 text-[12px] text-ink-3">
            <span>Pairing is on for another {minutes}.</span>
            <Button size="sm" onClick={() => void flip.run(api.hostDisarmPairing)} busy={flip.busy}>
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
            <Button tone="primary" onClick={() => void flip.run(api.hostArmPairing)} busy={flip.busy}>
              Turn pairing on for 10 minutes
            </Button>
          </div>
        </>
      )}
      {pairing.note ? <div className="mt-3 text-[13px] font-medium text-good">{pairing.note}</div> : null}
      <ErrorLine error={flip.error} className="mt-3" />
    </Card>
  );
}

/** Who uses this computer: everyone that paired, and whoever has an ssh
 * session open right now (a running stack, a copy, a terminal). */
export function UsersSection({ host }: { host: HostSnapshot }) {
  return (
    <Card title="Computers using this one">
      <div className="text-[12px] font-semibold text-ink-2">Connected now</div>
      {host.connected.length === 0 ? <p className="mt-1 text-[13px] text-ink-2">No computer is connected right now.</p> : null}
      <div className="mt-1 space-y-1">
        {host.connected.map((computer) => (
          <div key={computer.address} className="flex items-center gap-3 text-[13px]">
            <StatusDot state="good" pulse label="connected" />
            <span className="font-medium text-ink">{computer.name || computer.address}</span>
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
          <PairedRow key={computer.address} computer={computer} />
        ))}
      </div>
      {host.paired.length > 0 ? (
        <p className="mt-2 text-[12px] text-ink-3">
          A paired computer keeps its access while sharing is off: its key stays on this computer. Forget takes it away.
        </p>
      ) : null}
    </Card>
  );
}

/** One paired computer, with Forget behind a confirmation. */
function PairedRow({ computer }: { computer: PairedComputer }) {
  const [asking, setAsking] = useState(false);
  const forgetting = useAction("inline");
  const name = computer.name || "another computer";
  const forget = () => {
    setAsking(false);
    void forgetting.run(() => api.hostForget(computer.address));
  };
  return (
    <div className="rounded-lg px-2 py-1 text-[13px] hover:bg-surface-2/60">
      <div className="flex items-center gap-3">
        <span className="font-medium text-ink">{name}</span>
        <span className="mono text-ink-3">{computer.address}</span>
        <span className="ml-auto text-ink-3">
          {computer.key_type} · paired {new Date(computer.paired_at_ms).toLocaleDateString()}
        </span>
        {asking ? (
          <>
            <Button size="sm" tone="ghost" onClick={() => setAsking(false)}>
              Keep
            </Button>
            <Button size="sm" tone="danger" onClick={forget}>
              Forget
            </Button>
          </>
        ) : (
          <Button size="sm" onClick={() => setAsking(true)} busy={forgetting.busy} title={`Remove ${name}'s key, so it can no longer log in here`}>
            Forget…
          </Button>
        )}
      </div>
      {computer.fingerprint ? <div className="mono selectable text-[11px] text-ink-3">{computer.fingerprint}</div> : null}
      {asking ? <div className="mt-1 text-[12px] text-ink-2">{name} can no longer log in here or run stacks. Pair again to give it access back.</div> : null}
      <ErrorLine error={forgetting.error} className="mt-1" />
    </div>
  );
}
