import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import type { HostOs, ScriptRequest, ServeInfo } from "../lib/types";
import { useStore } from "../state/store";
import { SpinnerIcon } from "../ui/icons";
import { Button, Chip, CodeBlock, Field, TextInput } from "../ui/primitives";

/** What may stand between the machine and the script this computer serves. */
const FIREWALL_HINT: Record<HostOs, string> = {
  macos: "If macOS asks whether dockerNanny may accept connections, allow it.",
  windows: "If Windows Firewall asks whether dockerNanny may use private networks, allow it.",
  linux: "If this computer runs a firewall, let the machine reach the port above.",
};

/** The app does steps 2 to 7: it serves a generated PowerShell script on the
 * LAN and the machine pulls it with one line. */
export function SetupScriptCard() {
  const os = useStore((state) => state.os);
  const fetched = useStore((state) => state.scriptFetched);
  const setScriptFetched = useStore((state) => state.setScriptFetched);
  const [keyPath, setKeyPath] = useState("");
  // The machine's memory is not known from here; 8 GB suits a 16 GB machine and is easy to change.
  const [memory, setMemory] = useState("8");
  const [distro, setDistro] = useState("Ubuntu");
  const [port, setPort] = useState("2222");
  const [keepAwake, setKeepAwake] = useState(true);
  const [makePrivate, setMakePrivate] = useState(false);
  const [serving, setServing] = useState<ServeInfo | null>(null);
  const [script, setScript] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api.defaultKeyPath().then(setKeyPath).catch(console.warn);
    return () => {
      void api.scriptStop().catch(() => {});
    };
  }, []);

  const request = (): ScriptRequest => ({
    key_path: keyPath.trim(),
    port: Number(port) || 2222,
    memory_gb: Number(memory) || 8,
    distro: distro.trim() || "Ubuntu",
    keep_awake: keepAwake,
    make_private: makePrivate,
  });

  const serve = async () => {
    setWorking(true);
    setError(null);
    setScriptFetched(null);
    try {
      setServing(await api.scriptServe(request()));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  };

  const stop = async () => {
    await api.scriptStop().catch(() => {});
    setServing(null);
  };

  const toggleScript = async () => {
    if (script) {
      setScript(null);
      return;
    }
    try {
      setScript(await api.scriptPreview(request()));
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <section className="mt-6 rounded-2xl border border-accent/40 bg-surface p-4">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-[14px] font-semibold text-ink">Let the app do steps 2 to 7</h2>
        <span className="text-[11px] text-ink-3">after step 1 is done</span>
      </div>
      <p className="mt-2 text-[12px] leading-relaxed text-ink-2">
        dockerNanny writes a PowerShell script with this computer's public key inside and hands it to the machine over the network. On the machine, open PowerShell as Administrator and paste
        one line. The script installs what is missing, skips what is there, and prints the address, user and port to enter here.
      </p>

      <div className="mt-4 grid grid-cols-2 gap-3">
        <Field label="This computer's key" hint="Its public half goes into authorized_keys">
          <TextInput value={keyPath} onChange={(e) => setKeyPath(e.target.value)} className="mono" disabled={serving !== null} />
        </Field>
        <Field label="WSL distro" hint="Installed on the machine in step 1">
          <TextInput value={distro} onChange={(e) => setDistro(e.target.value)} disabled={serving !== null} />
        </Field>
        <Field label="RAM for Docker (GB)" hint="About half of the machine's memory">
          <TextInput value={memory} onChange={(e) => setMemory(e.target.value.replace(/\D/g, ""))} inputMode="numeric" disabled={serving !== null} />
        </Field>
        <Field label="SSH port" hint="Away from a Windows SSH server on 22">
          <TextInput value={port} onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))} inputMode="numeric" disabled={serving !== null} />
        </Field>
      </div>
      <div className="mt-3 space-y-1.5 text-[12px] text-ink-2">
        <label className="flex items-start gap-2">
          <input type="checkbox" checked={keepAwake} onChange={(e) => setKeepAwake(e.target.checked)} disabled={serving !== null} className="mt-0.5 accent-accent" />
          <span>Keep the machine awake while plugged in, lid closed included (changes its power settings)</span>
        </label>
        <label className="flex items-start gap-2">
          <input type="checkbox" checked={makePrivate} onChange={(e) => setMakePrivate(e.target.checked)} disabled={serving !== null} className="mt-0.5 accent-accent" />
          <span>Mark its network Private if Windows has it as Public (only on a network you trust)</span>
        </label>
      </div>

      {serving ? (
        <div className="mt-4 space-y-2">
          <div className="text-[12px] text-ink-2">On the machine, in PowerShell as Administrator:</div>
          {serving.addresses.map((address) => (
            <CodeBlock key={address} code={`irm http://${address}:${serving.port}/setup.ps1 | iex`} />
          ))}
          {serving.addresses.length > 1 ? <div className="text-[11px] text-ink-3">One line per network this computer is on; use the one the machine shares.</div> : null}
          <div className="flex items-center gap-2 text-[12px]">
            {fetched ? (
              <Chip tone="good">fetched by {fetched.from}</Chip>
            ) : (
              <Chip tone="accent">
                <SpinnerIcon size={10} /> waiting for the machine
              </Chip>
            )}
            <span className="text-ink-3">{fetched ? "The script is running there; watch its output, then click Add machine." : FIREWALL_HINT[os]}</span>
          </div>
        </div>
      ) : null}

      {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}

      <div className="mt-4 flex items-center justify-between">
        <Button tone="ghost" size="sm" onClick={() => void toggleScript()}>
          {script ? "Hide the script" : "Show the script"}
        </Button>
        {serving ? (
          <Button onClick={() => void stop()}>Stop serving the script</Button>
        ) : (
          <Button tone="primary" onClick={() => void serve()} busy={working} disabled={!keyPath.trim()}>
            Share the setup script
          </Button>
        )}
      </div>
      {script ? (
        <div className="mt-3">
          <CodeBlock code={script} />
        </div>
      ) : null}
    </section>
  );
}
