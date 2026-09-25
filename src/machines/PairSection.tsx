import { useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine } from "../lib/types";
import { useStore } from "../state/store";
import { SpinnerIcon } from "../ui/icons";
import { Button, Field, TextInput } from "../ui/primitives";

/** The machine shows a pairing code (dockerNanny with sharing on): type what its screen shows, done. */
export function PairSection({ keyPath, onPaired }: { keyPath: string; onPaired: (machine: Machine) => void }) {
  const [address, setAddress] = useState("");
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const setMachines = useStore((state) => state.setMachines);

  // A first-time user often has no key; the fix is one click, so it sits next to the error.
  const noKey = error?.startsWith("No key file") ?? false;
  const createKey = async () => {
    setWorking(true);
    try {
      await api.generateKey();
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  };

  const pair = async () => {
    setWorking(true);
    setError(null);
    try {
      const machine = await api.pairMachine(address, code, keyPath, name);
      setMachines(await api.listMachines());
      onPaired(machine);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="rounded-xl border border-accent/40 bg-accent-soft/40 p-4">
      <div className="text-[13px] font-medium text-ink">Machine showing a pairing code</div>
      <p className="mt-1 text-[12px] text-ink-2">Type the address and the six digit code from the machine's sharing page. The user, port and host key come from the machine; nothing else to fill in.</p>
      <div className="mt-3 grid grid-cols-3 gap-3">
        <Field label="Address">
          <TextInput value={address} onChange={(e) => setAddress(e.target.value)} placeholder="192.0.2.15" autoFocus />
        </Field>
        <Field label="Code">
          <TextInput value={code} onChange={(e) => setCode(e.target.value.replace(/\D/g, "").slice(0, 6))} placeholder="000000" inputMode="numeric" className="tabular" />
        </Field>
        <Field label="Name" hint="Optional">
          <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="workshop" />
        </Field>
      </div>
      {error ? <div className="selectable mt-2 text-[12px] text-critical">{error}</div> : null}
      {noKey ? (
        <div className="mt-2 flex items-center justify-between gap-3 rounded-lg border border-line bg-surface px-3 py-2 text-[12px] text-ink-2">
          <span>This computer has no SSH key yet. Make one here, then pair again.</span>
          <Button size="sm" onClick={() => void createKey()} disabled={working}>
            Create a key
          </Button>
        </div>
      ) : null}
      <div className="mt-3 flex justify-end">
        <Button tone="primary" onClick={() => void pair()} disabled={working || !address.trim() || code.length !== 6}>
          {working ? <SpinnerIcon /> : null} Pair
        </Button>
      </div>
    </div>
  );
}
