import { useState } from "react";

import { api } from "../lib/ipc";
import { DoctorRows } from "../machines/DoctorRows";
import { useStore } from "../state/store";
import { RefreshIcon } from "../ui/icons";
import { Button, Card, ErrorLine, Inset } from "../ui/primitives";
import { useAction } from "../ui/useAction";
import { useLoaded } from "../ui/useLoaded";

/** What this computer needs before it can use a machine, in checking order.
 * On Windows, ssh and rsync run inside WSL, so WSL comes first. */
const READINESS_CHECKS: Array<{ key: string; label: string }> = [
  { key: "ssh", label: "SSH client" },
  { key: "rsync", label: "rsync" },
  { key: "key", label: "SSH key" },
  { key: "docker", label: "Docker here" },
];
const WSL_CHECK = { key: "wsl", label: "WSL" };

/** The controller role's own doctor: the tools and the key this computer
 * needs to reach a machine. A missing key can be made right here. */
export function Readiness() {
  const os = useStore((state) => state.os);
  const checks = os === "windows" ? [WSL_CHECK, ...READINESS_CHECKS] : READINESS_CHECKS;
  const readiness = useLoaded(api.computerReadiness);
  const creating = useAction("inline");
  const installing = useAction("inline");
  const [note, setNote] = useState<string | null>(null);

  const rows = readiness.data ?? [];
  const checking = readiness.loading;

  const createKey = async () => {
    const made = await creating.run(async () => {
      const publicKey = await api.generateKey();
      setNote(`Created. The public half is ${publicKey}; pairing installs it on a machine for you.`);
    });
    if (made) void readiness.reload();
  };

  const installTools = async () => {
    const installed = await installing.run(api.installWslTools);
    if (!installed) return;
    setNote("ssh and rsync are installed inside WSL.");
    void readiness.reload();
  };

  const isOk = (key: string) => rows.some((row) => row.key === key && row.ok);
  const keyMissing = rows.some((row) => row.key === "key" && !row.ok);
  // On Windows the tools live in WSL, where the app can install them without an administrator prompt.
  const toolsMissing = !isOk("ssh") || !isOk("rsync");
  const canInstallTools = os === "windows" && !checking && isOk("wsl") && toolsMissing;
  const allReady = !checking && rows.length > 0 && rows.every((row) => row.ok);

  return (
    <Card
      title="Ready to use other machines?"
      description={allReady ? "Everything this computer needs is here." : "What this computer needs to reach a machine over ssh."}
      actions={
        <Button size="sm" tone="ghost" onClick={() => void readiness.reload()} busy={checking} aria-label="Check again">
          <RefreshIcon />
        </Button>
      }
    >
      <DoctorRows rows={rows} checking={checking} checks={checks} fixesAreCommands={false} />
      {canInstallTools ? (
        <Inset className="mt-3 flex flex-wrap items-center justify-between gap-3">
          <span className="text-[12px] text-ink-2">
            Installs openssh-client and rsync inside the WSL distribution with apt-get, as its root user. Windows is not changed.
          </span>
          <Button tone="primary" onClick={() => void installTools()} busy={installing.busy}>
            Install in WSL
          </Button>
        </Inset>
      ) : null}
      {keyMissing ? (
        <Inset className="mt-3 flex flex-wrap items-center justify-between gap-3">
          <span className="text-[12px] text-ink-2">A new ed25519 key without a passphrase, saved where the row above says. Nothing leaves this computer.</span>
          <Button tone="primary" onClick={() => void createKey()} busy={creating.busy}>
            Create a key
          </Button>
        </Inset>
      ) : null}
      {note ? <div className="mt-3 text-[12px] text-good">{note}</div> : null}
      <ErrorLine error={readiness.error ?? creating.error ?? installing.error} className="mt-3" />
    </Card>
  );
}
