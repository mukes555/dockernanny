import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine, TerminalInfo } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { SpinnerIcon } from "../ui/icons";
import { Button, CodeBlock } from "../ui/primitives";

/** The same machine from a terminal: the ssh alias, and an optional Docker
 * context. On Windows ssh runs inside WSL, so the commands do too, and the
 * Docker context is not offered yet. */
export function TerminalDialog({ machine, onClose }: { machine: Machine | null; onClose: () => void }) {
  const appHome = useStore((state) => state.appHome);
  const setMachines = useStore((state) => state.setMachines);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<TerminalInfo | null>(null);

  useEffect(() => {
    if (!machine) return;
    void api
      .terminalInfo()
      .then(setInfo)
      .catch(() => setInfo(null));
  }, [machine]);

  const toggle = async (enabled: boolean) => {
    if (!machine) return;
    setWorking(true);
    setError(null);
    try {
      setMachines(await api.setDockerContext(machine.id, enabled));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  };

  const config = info?.ssh_config ?? `${appHome || "~/.dockernanny"}/ssh_config`;
  const wsl = info?.wsl_distro ?? null;
  const contextName = machine ? `dn-${contextSlug(machine.name)}` : "";
  if (machine && wsl) {
    return (
      <Dialog open onClose={onClose} eyebrow="Terminal" title={`Use ${machine.name} from your terminal`} width={520}>
        <div className="mt-3 space-y-4 text-[13px] text-ink-2">
          <p>On Windows, dockerNanny runs ssh inside WSL ({wsl}). From PowerShell or any terminal:</p>
          <CodeBlock code={`wsl -d ${wsl} ssh -F ${config} dn-${machine.id}`} />
          <p>Inside {wsl}, one line at the top of its ~/.ssh/config makes the short form work there:</p>
          <CodeBlock code={`Include ${config}`} />
          <CodeBlock code={`ssh dn-${machine.id}`} />
          <p className="text-[12px] text-ink-3">A Docker context for this machine is not available on Windows yet.</p>
          <div className="flex justify-end pt-1">
            <Button tone="ghost" onClick={onClose}>
              Close
            </Button>
          </div>
        </div>
      </Dialog>
    );
  }
  return (
    <Dialog open={machine !== null} onClose={onClose} eyebrow="Terminal" title={machine ? `Use ${machine.name} from your terminal` : ""} width={520}>
      {machine ? (
        <div className="mt-3 space-y-4 text-[13px] text-ink-2">
          <p>dockerNanny keeps its ssh settings in its own file and never edits yours. Add one line at the top of ~/.ssh/config and the alias works everywhere:</p>
          <CodeBlock code={`Include ${config}`} />
          <CodeBlock code={`ssh dn-${machine.id}`} />
          <p>
            With that line in place, a Docker context lets any terminal talk to the machine's engine directly.{" "}
            {machine.docker_context ? (
              <>
                The context <span className="mono text-ink">{contextName}</span> exists.
              </>
            ) : null}
          </p>
          {machine.docker_context ? (
            <CodeBlock code={`docker context use ${contextName}\ndocker ps\n# back to this computer's engine:\ndocker context use default`} />
          ) : null}
          {error ? <div className="text-[12px] text-critical">{error}</div> : null}
          <div className="flex items-center justify-between pt-1">
            <Button tone="ghost" onClick={onClose}>
              Close
            </Button>
            {machine.docker_context ? (
              <Button tone="danger" disabled={working} onClick={() => void toggle(false)}>
                {working ? <SpinnerIcon /> : null} Remove the context
              </Button>
            ) : (
              <Button tone="primary" disabled={working} onClick={() => void toggle(true)}>
                {working ? <SpinnerIcon /> : null} Create context {contextName}
              </Button>
            )}
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}

/** Mirrors the backend's context naming so the dialog can show it before creation. */
export function contextSlug(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, "-")
    .replace(/^[-_]+|[-_]+$/g, "");
  // The same fallback as the backend's sanitize_name.
  return slug || "stack";
}
