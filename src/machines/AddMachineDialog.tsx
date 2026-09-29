import { useEffect, useState } from "react";

import { api } from "../lib/ipc";
import type { Machine, PairedMachine } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, ErrorLine, Eyebrow, Field, Inset, TextInput } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import { DoctorRows } from "./DoctorRows";
import { PairSection } from "./PairSection";

const NO_ROWS: never[] = [];

/** Pair with a machine that shows a code, or describe one by hand, check it
 * live, and add it once SSH works. The form mounts with the dialog, so every
 * opening starts empty with a fresh id. */
export function AddMachineDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onClose={onClose} eyebrow="Machine" title="Add a machine" width={560} closeOnBackdrop={false}>
      <AddMachineForm onClose={onClose} />
    </Dialog>
  );
}

function AddMachineForm({ onClose }: { onClose: () => void }) {
  const setMachines = useStore((state) => state.setMachines);
  const resetDoctor = useStore((state) => state.resetDoctor);
  const [draftId, setDraftId] = useState("");
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [user, setUser] = useState("");
  const [port, setPort] = useState("22");
  const [keyPath, setKeyPath] = useState("");
  const [checked, setChecked] = useState(false);
  /** Set once a machine paired: the machine is already saved, only the check is left. */
  const [pairing, setPairing] = useState<PairedMachine | null>(null);
  const check = useAction("inline");
  const adding = useAction("inline");
  const paired = pairing?.machine ?? null;
  const rows = useStore((state) => state.doctor[paired?.id ?? draftId] ?? NO_ROWS);
  const sshOk = rows.some((row) => row.key === "ssh" && row.ok);

  useEffect(() => {
    api.newMachineId().then(setDraftId).catch(console.warn);
    api.defaultKeyPath().then(setKeyPath).catch(console.warn);
  }, []);

  const draft = (): Machine =>
    paired ?? {
      id: draftId,
      name: name.trim(),
      user: user.trim(),
      host: host.trim(),
      port: Number(port) || 22,
      key_path: keyPath.trim(),
      docker_context: false,
      pinned: false,
    };

  // A check answers for the details it was run with; changing one of them
  // clears the answer, so a machine is never added on an old "passed".
  const afterEdit = (setter: (value: string) => void) => (value: string) => {
    setter(value);
    if (checked || rows.length > 0) {
      setChecked(false);
      resetDoctor(draftId);
    }
  };

  const runCheck = async (machine: Machine) => {
    resetDoctor(machine.id);
    const passed = await check.run(() => api.doctor(machine));
    if (passed) setChecked(true);
  };

  const onPaired = (result: PairedMachine) => {
    setPairing(result);
    void runCheck(result.machine);
  };

  const add = async () => {
    const added = await adding.run(async () => setMachines(await api.addMachine(draft())));
    if (!added) return;
    resetDoctor(draftId);
    onClose();
  };

  const browseKey = async () => {
    const chosen = await api.pickKeyFile().catch(() => null);
    if (chosen) afterEdit(setKeyPath)(chosen);
  };

  const hasAddress = host.trim() !== "" && user.trim() !== "";
  const hasName = name.trim() !== "";

  return (
    <>
      {paired ? (
        <>
          <p className="mt-1 text-[13px] text-ink-2">
            Paired with <span className="font-medium text-ink">{paired.name}</span> ({paired.user}@{paired.host}:{paired.port}). Checking it now.
          </p>
          <Fingerprints hostKey={pairing?.host_fingerprint ?? null} ownKey={pairing?.key_fingerprint ?? null} />
        </>
      ) : (
        <>
          <div className="mt-4">
            <PairSection keyPath={keyPath} onPaired={onPaired} />
          </div>
          <Eyebrow className="mt-6">Or any Linux machine by hand</Eyebrow>
          <p className="mt-1 text-[12px] text-ink-2">Anything with sshd and Docker that already accepts your key.</p>
        </>
      )}
      <div className={paired ? "hidden" : "mt-4 grid grid-cols-2 gap-3"}>
        <Field label="Name">
          <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="workshop" />
        </Field>
        <Field label="Address">
          <TextInput value={host} onChange={(e) => afterEdit(setHost)(e.target.value)} placeholder="192.0.2.15 or studio.local" />
        </Field>
        <Field label="User">
          <TextInput value={user} onChange={(e) => afterEdit(setUser)(e.target.value)} placeholder="alex" />
        </Field>
        <Field label="Port" hint="2222 for a Windows machine set up with the guide">
          <TextInput value={port} onChange={(e) => afterEdit(setPort)(e.target.value.replace(/\D/g, "").slice(0, 5))} inputMode="numeric" />
        </Field>
      </div>
      <div className={paired ? "hidden" : "mt-3"}>
        <Field label="Private key" hint="Pairing sends its .pub to the machine. For a manual add it must already be on the machine (ssh-copy-id).">
          <div className="flex gap-2">
            <TextInput value={keyPath} onChange={(e) => afterEdit(setKeyPath)(e.target.value)} className="mono" />
            <Button onClick={() => void browseKey()}>Browse</Button>
          </div>
        </Field>
      </div>

      {rows.length > 0 || check.busy ? (
        <div className="mt-5">
          <DoctorRows rows={rows} checking={check.busy} />
        </div>
      ) : null}

      <ErrorLine error={check.error ?? adding.error} className="mt-3" />

      <div className="mt-6 flex items-center justify-between gap-2">
        <Button tone="ghost" onClick={onClose}>
          {paired ? "Close" : "Cancel"}
        </Button>
        {paired ? (
          <Button tone="primary" onClick={onClose} busy={check.busy}>
            Done
          </Button>
        ) : (
          <div className="flex gap-2">
            <Button onClick={() => void runCheck(draft())} busy={check.busy} disabled={!hasAddress}>
              {checked ? "Check again" : "Check connection"}
            </Button>
            <Button tone="primary" onClick={() => void add()} busy={adding.busy} disabled={!sshOk || check.busy || !hasName} title={whyNotYet(sshOk, hasName)}>
              Add machine
            </Button>
          </div>
        )}
      </div>
    </>
  );
}

/** What Add machine waits for, said on hover. */
function whyNotYet(sshOk: boolean, hasName: boolean): string | undefined {
  if (!sshOk) return "Run the connection check first";
  if (!hasName) return "Give it a name";
  return undefined;
}

/** What the machine's sharing page shows too, so the two screens can be
 * compared: a key swapped by someone in between would not match. */
function Fingerprints({ hostKey, ownKey }: { hostKey: string | null; ownKey: string | null }) {
  if (!hostKey && !ownKey) return null;
  return (
    <Inset className="mt-3 text-[12px] text-ink-2">
      <div>The machine's screen shows the same two keys. If one differs, remove this machine: someone may be in between.</div>
      {hostKey ? (
        <div className="mt-1">
          The machine's host key <span className="mono selectable text-ink">{hostKey}</span>
        </div>
      ) : null}
      {ownKey ? (
        <div>
          This computer's key <span className="mono selectable text-ink">{ownKey}</span>
        </div>
      ) : null}
    </Inset>
  );
}
