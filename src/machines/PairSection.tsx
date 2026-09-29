import { useEffect, useState } from "react";

import { api } from "../lib/ipc";
import type { PairedMachine } from "../lib/types";
import { useStore } from "../state/store";
import { Button, ErrorLine, Field, TextInput } from "../ui/primitives";
import { useAction } from "../ui/useAction";

/** The machine shows a pairing code (dockerNanny with sharing on): type what its screen shows, done. */
export function PairSection({ keyPath, onPaired }: { keyPath: string; onPaired: (paired: PairedMachine) => void }) {
  const [address, setAddress] = useState("");
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  // null until known; pairing sends this key's public half, so without one there is nothing to pair with.
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const pairing = useAction("inline");
  const making = useAction("inline");
  const setMachines = useStore((state) => state.setMachines);

  useEffect(() => {
    if (!keyPath) return;
    api.keyExists(keyPath).then(setHasKey).catch(console.warn);
  }, [keyPath]);

  const createKey = async () => {
    const made = await making.run(api.generateKey);
    if (made) setHasKey(true);
  };

  const pair = () =>
    pairing.run(async () => {
      const paired = await api.pairMachine(address, code, keyPath, name);
      setMachines(await api.listMachines());
      onPaired(paired);
    });

  const ready = address.trim() !== "" && code.length === 6 && hasKey !== false;

  return (
    <div className="rounded-xl border border-accent/40 bg-accent-soft/40 p-4">
      <div className="text-[13px] font-medium text-ink">Machine showing a pairing code</div>
      <p className="mt-1 text-[12px] text-ink-2">
        Type the address and the six digit code from the machine's sharing page. The user, port and host key come from the machine; nothing else to fill in.
      </p>
      <div className="mt-3 grid grid-cols-3 gap-3">
        <Field label="Address">
          <TextInput value={address} onChange={(e) => setAddress(e.target.value)} placeholder="192.0.2.15" autoFocus />
        </Field>
        <Field label="Code">
          <TextInput
            value={code}
            onChange={(e) => setCode(e.target.value.replace(/\D/g, "").slice(0, 6))}
            placeholder="000000"
            inputMode="numeric"
            className="tabular"
          />
        </Field>
        <Field label="Name" hint="Optional">
          <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="workshop" />
        </Field>
      </div>
      {hasKey === false ? (
        <div className="mt-3 flex items-center justify-between gap-3 rounded-lg border border-line bg-surface px-3 py-2 text-[12px] text-ink-2">
          <span>This computer has no SSH key yet, and pairing sends its public half. Make one here first.</span>
          <Button size="sm" onClick={() => void createKey()} busy={making.busy}>
            Create a key
          </Button>
        </div>
      ) : null}
      <ErrorLine error={making.error ?? pairing.error} />
      <div className="mt-3 flex justify-end">
        <Button tone="primary" onClick={() => void pair()} busy={pairing.busy} disabled={!ready}>
          Pair
        </Button>
      </div>
    </div>
  );
}
