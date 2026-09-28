import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, Eyebrow, Field, TextInput } from "../ui/primitives";
import { DoctorRows } from "./DoctorRows";
import { PairSection } from "./PairSection";

/** Describe a machine, check it live, add it once SSH works. */
export function AddMachineDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const setMachines = useStore((state) => state.setMachines);
  const resetDoctor = useStore((state) => state.resetDoctor);
  const doctorRows = useStore((state) => state.doctor);
  const [draftId, setDraftId] = useState("");
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [user, setUser] = useState("");
  const [port, setPort] = useState("22");
  const [keyPath, setKeyPath] = useState("");
  const [checking, setChecking] = useState(false);
  const [checked, setChecked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Set once a machine paired: the machine is already saved, only the doctor is left. */
  const [paired, setPaired] = useState<Machine | null>(null);

  // A fresh id and the default key every time the dialog opens.
  useEffect(() => {
    if (!open) return;
    setChecked(false);
    setError(null);
    setPaired(null);
    void api.newMachineId().then(setDraftId).catch(console.warn);
    void api.defaultKeyPath().then(setKeyPath).catch(console.warn);
  }, [open]);

  const rows = doctorRows[paired?.id ?? draftId] ?? [];
  const sshOk = rows.some((row) => row.key === "ssh" && row.ok);
  const draft = (): Machine => paired ?? { id: draftId, name: name.trim(), user: user.trim(), host: host.trim(), port: Number(port) || 22, key_path: keyPath.trim(), docker_context: false, pinned: false };

  // A check answers for the details it was run with; changing one of them
  // clears the answer, so a machine is never added on an old "passed".
  const afterEdit = (setter: (value: string) => void) => (value: string) => {
    setter(value);
    if (checked || rows.length > 0) {
      setChecked(false);
      resetDoctor(draftId);
    }
  };

  const onPaired = (machine: Machine) => {
    setPaired(machine);
    setChecking(true);
    resetDoctor(machine.id);
    void api
      .doctor(machine)
      .then(() => setChecked(true))
      .catch((err) => setError(errorMessage(err)))
      .finally(() => setChecking(false));
  };

  const check = async () => {
    setError(null);
    setChecking(true);
    resetDoctor(draftId);
    try {
      await api.doctor(draft());
      setChecked(true);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setChecking(false);
    }
  };

  const add = async () => {
    setError(null);
    try {
      setMachines(await api.addMachine(draft()));
      resetDoctor(draftId);
      setName("");
      setHost("");
      setUser("");
      setPort("22");
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const browseKey = async () => {
    const chosen = await api.pickKeyFile().catch(() => null);
    if (chosen) afterEdit(setKeyPath)(chosen);
  };

  return (
    <Dialog open={open} onClose={onClose} eyebrow="Machine" title="Add a machine" width={560} closeOnBackdrop={false}>
      {paired ? (
        <p className="mt-1 text-[13px] text-ink-2">
          Paired with <span className="font-medium text-ink">{paired.name}</span> ({paired.user}@{paired.host}:{paired.port}). Checking it now.
        </p>
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

      {rows.length > 0 || checking ? (
        <div className="mt-5">
          <DoctorRows rows={rows} checking={checking} />
        </div>
      ) : null}

      {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}

      <div className="mt-6 flex items-center justify-between gap-2">
        <Button tone="ghost" onClick={onClose}>
          {paired ? "Close" : "Cancel"}
        </Button>
        {paired ? (
          <Button tone="primary" onClick={onClose} busy={checking}>
            Done
          </Button>
        ) : (
          <div className="flex gap-2">
            <Button onClick={() => void check()} busy={checking} disabled={!host.trim() || !user.trim()}>
              {checked ? "Check again" : "Check connection"}
            </Button>
            <Button tone="primary" onClick={() => void add()} disabled={!sshOk || checking || !name.trim()} title={!sshOk ? "Run the connection check first" : !name.trim() ? "Give it a name" : undefined}>
              Add machine
            </Button>
          </div>
        )}
      </div>
    </Dialog>
  );
}

