import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { Machine, TerminalInfo } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { Button, CodeBlock, ErrorLine } from "../ui/primitives";
import { useAction } from "../ui/useAction";

/** The backend's answer for one machine: its names, or why they could not be read. */
interface Answer {
  machineId: string;
  info: TerminalInfo | null;
  error: string | null;
}

/** The same machine from a terminal: the ssh alias, and an optional Docker
 * context. On Windows ssh runs inside WSL, so the commands do too, and the
 * Docker context is not offered yet. The names come from the backend, which
 * writes the ssh config and makes the context. */
export function TerminalDialog({ machine, onClose }: { machine: Machine | null; onClose: () => void }) {
  const [answer, setAnswer] = useState<Answer | null>(null);

  useEffect(() => {
    if (!machine) return;
    const machineId = machine.id;
    api
      .terminalInfo(machineId)
      .then((info) => setAnswer({ machineId, info, error: null }))
      .catch((err) => setAnswer({ machineId, info: null, error: errorMessage(err) }));
  }, [machine]);

  // An answer for another machine is never shown for this one.
  const current = answer && machine && answer.machineId === machine.id ? answer : null;
  return (
    <Dialog open={machine !== null} onClose={onClose} eyebrow="Terminal" title={machine ? `Use ${machine.name} from your terminal` : ""} width={520}>
      {machine && current?.info ? (
        <TerminalHowTo machine={machine} info={current.info} onClose={onClose} />
      ) : (
        <NotReadYet error={current?.error ?? null} onClose={onClose} />
      )}
    </Dialog>
  );
}

/** While the names are read, or when they could not be: the dialog always
 * says what it is waiting for and always has a way out. */
function NotReadYet({ error, onClose }: { error: string | null; onClose: () => void }) {
  return (
    <div className="mt-3 space-y-4 text-[13px] text-ink-2">
      {error ? (
        <ErrorLine error={`The terminal details could not be read: ${error}`} className="mt-0" />
      ) : (
        <p className="text-ink-3">Reading the machine's ssh alias…</p>
      )}
      <div className="flex justify-end pt-1">
        <Button tone="ghost" onClick={onClose}>
          Close
        </Button>
      </div>
    </div>
  );
}

function TerminalHowTo({ machine, info, onClose }: { machine: Machine; info: TerminalInfo; onClose: () => void }) {
  const setMachines = useStore((state) => state.setMachines);
  const toggle = useAction("inline");
  const setContext = (enabled: boolean) => toggle.run(async () => setMachines(await api.setDockerContext(machine.id, enabled)));

  if (info.wsl_distro) {
    const wsl = info.wsl_distro;
    return (
      <div className="mt-3 space-y-4 text-[13px] text-ink-2">
        <p>On Windows, dockerNanny runs ssh inside WSL ({wsl}). From PowerShell or any terminal:</p>
        <CodeBlock code={`wsl -d ${wsl} ssh -F ${info.ssh_config} ${info.alias}`} />
        <p>Inside {wsl}, one line at the top of its ~/.ssh/config makes the short form work there:</p>
        <CodeBlock code={`Include ${info.ssh_config}`} />
        <CodeBlock code={`ssh ${info.alias}`} />
        <p className="text-[12px] text-ink-3">A Docker context for this machine is not available on Windows yet.</p>
        <div className="flex justify-end pt-1">
          <Button tone="ghost" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="mt-3 space-y-4 text-[13px] text-ink-2">
      <p>dockerNanny keeps its ssh settings in its own file and never edits yours. Add one line at the top of ~/.ssh/config and the alias works everywhere:</p>
      <CodeBlock code={`Include ${info.ssh_config}`} />
      <CodeBlock code={`ssh ${info.alias}`} />
      <p>
        With that line in place, a Docker context lets any terminal talk to the machine's engine directly.{" "}
        {machine.docker_context ? (
          <>
            The context <span className="mono text-ink">{info.context_name}</span> exists.
          </>
        ) : null}
      </p>
      {machine.docker_context ? (
        <CodeBlock code={`docker context use ${info.context_name}\ndocker ps\n# back to this computer's engine:\ndocker context use default`} />
      ) : null}
      <ErrorLine error={toggle.error} className="mt-0" />
      <div className="flex items-center justify-between pt-1">
        <Button tone="ghost" onClick={onClose}>
          Close
        </Button>
        {machine.docker_context ? (
          <Button tone="danger" busy={toggle.busy} onClick={() => void setContext(false)}>
            Remove the context
          </Button>
        ) : (
          <Button tone="primary" busy={toggle.busy} onClick={() => void setContext(true)}>
            Create context {info.context_name}
          </Button>
        )}
      </div>
    </div>
  );
}
