import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { DoctorRow } from "../lib/types";
import { DoctorRows } from "../machines/DoctorRows";
import { RefreshIcon, SpinnerIcon } from "../ui/icons";
import { Button, Card } from "../ui/primitives";

/** What this computer needs before it can use a machine, in checking order. */
const READINESS_CHECKS: Array<{ key: string; label: string }> = [
  { key: "ssh", label: "SSH client" },
  { key: "rsync", label: "rsync" },
  { key: "key", label: "SSH key" },
  { key: "docker", label: "Docker here" },
];

/** The controller role's own doctor: the tools and the key this computer
 * needs to reach a machine. A missing key can be made right here. */
export function Readiness() {
  const [rows, setRows] = useState<DoctorRow[]>([]);
  const [checking, setChecking] = useState(true);
  const [creating, setCreating] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const check = () => {
    setChecking(true);
    setError(null);
    api
      .computerReadiness()
      .then(setRows)
      .catch((err) => setError(errorMessage(err)))
      .finally(() => setChecking(false));
  };
  useEffect(check, []);

  const createKey = async () => {
    setCreating(true);
    setError(null);
    try {
      const publicKey = await api.generateKey();
      setNote(`Created. The public half is ${publicKey}; pairing installs it on a machine for you.`);
      check();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setCreating(false);
    }
  };

  const keyMissing = rows.some((row) => row.key === "key" && !row.ok);
  const allReady = !checking && rows.length > 0 && rows.every((row) => row.ok);

  return (
    <Card
      title="Ready to use other machines?"
      description={allReady ? "Everything this computer needs is here." : "What this computer needs to reach a machine over ssh."}
      actions={
        <Button size="sm" tone="ghost" onClick={check} disabled={checking} aria-label="Check again">
          {checking ? <SpinnerIcon /> : <RefreshIcon />}
        </Button>
      }
    >
      <DoctorRows rows={rows} checking={checking} checks={READINESS_CHECKS} fixesAreCommands={false} />
      {keyMissing ? (
        <div className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-line bg-surface-2/40 px-3 py-2.5">
          <span className="text-[12px] text-ink-2">A new ed25519 key without a passphrase, saved where the row above says. Nothing leaves this computer.</span>
          <Button tone="primary" onClick={() => void createKey()} disabled={creating}>
            {creating ? <SpinnerIcon /> : null} Create a key
          </Button>
        </div>
      ) : null}
      {note ? <div className="mt-3 text-[12px] text-good">{note}</div> : null}
      {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}
    </Card>
  );
}
