import { useEffect, useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import { hiddenMachines } from "../lib/machines";
import type { Settings, Theme } from "../lib/types";
import { useStore } from "../state/store";
import { SpinnerIcon } from "../ui/icons";
import { Button, Card, Field, PageHeader, Select, TextInput, Toggle } from "../ui/primitives";

/** Mirrors `pairing::PORT` in the backend. */
const DEFAULT_PAIRING_PORT = 47433;

/** Every choice is saved as soon as it is made; text fields save when the
 * user leaves them, so a half-typed path never lands in the file. */
export function SettingsPage() {
  const settings = useStore((state) => state.settings);
  const os = useStore((state) => state.os);
  const saveSettings = useStore((state) => state.saveSettings);
  const appHome = useStore((state) => state.appHome);
  const setView = useStore((state) => state.setView);
  const [draft, setDraft] = useState<Settings | null>(settings);
  const [error, setError] = useState<string | null>(null);
  const [resetting, setResetting] = useState(false);
  const [resetNote, setResetNote] = useState<string | null>(null);

  useEffect(() => setDraft(settings), [settings]);

  if (!draft) {
    return (
      <div className="flex items-center gap-2 text-[13px] text-ink-2">
        <SpinnerIcon /> Loading settings…
      </div>
    );
  }

  const commit = (change: Partial<Settings>) => {
    const next = { ...draft, ...change };
    setDraft(next);
    void saveSettings(next).catch((err) => setError(errorMessage(err)));
  };
  const browseKey = async () => {
    const chosen = await api.pickKeyFile().catch(() => null);
    if (chosen) commit({ key_path: chosen });
  };
  const resetForwards = async () => {
    setResetting(true);
    setResetNote(null);
    try {
      await api.resetForwards();
      setResetNote("Every bridge was dropped; running stacks bring theirs back within a few seconds.");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setResetting(false);
    }
  };

  return (
    <div className="mx-auto max-w-2xl space-y-4 pb-10">
      <PageHeader eyebrow="This computer" title="Settings" onBack={() => setView("stacks")} />
      {error ? <div className="text-[12px] text-critical">{error}</div> : null}

      <Card title="Roles" description="Both can be on at the same time.">
        <div className="space-y-3">
          <RoleRow checked={draft.use_machines} onChange={(on) => commit({ use_machines: on })} label="Use other machines" hint="Send your compose stacks to other machines and keep using localhost here." />
          <RoleRow checked={draft.share_this_computer} onChange={(on) => commit({ share_this_computer: on })} label="Share this computer" hint="Let other computers run their stacks here. The sharing page sets this computer up and shows the pairing code." />
        </div>
      </Card>

      <Card title="Appearance">
        <Field label="Theme">
          <Select value={draft.theme} onChange={(e) => commit({ theme: e.target.value as Theme })}>
            <option value="system">Follow the system</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </Select>
        </Field>
      </Card>

      {os === "windows" ? <WslCard draft={draft} onCommit={commit} /> : null}
      <Card title="Updates" description="Only the version file of the latest GitHub release is fetched; nothing is installed until you choose to.">
        <Toggle checked={draft.check_updates} onChange={(on) => commit({ check_updates: on })} label="Look for a new version at start and every hour" />
      </Card>

      {draft.use_machines ? (
        <Card title="Using other machines" description="Defaults for new machines and stacks; each one can still differ.">
          <div className="space-y-3">
            <Field label="Private key" hint="Offered when adding a machine. Empty means the first key found in ~/.ssh.">
              <div className="flex gap-2">
                <TextInput value={draft.key_path} onChange={(e) => setDraft({ ...draft, key_path: e.target.value })} onBlur={() => commit({ key_path: draft.key_path })} className="mono" placeholder="~/.ssh/id_ed25519" />
                <Button onClick={() => void browseKey()}>Browse</Button>
              </div>
            </Field>
            <ExcludesField value={draft.excludes} onCommit={(excludes) => commit({ excludes })} />
            <HelperImageField value={draft.helper_image} onCommit={(helper_image) => commit({ helper_image })} />
            <div className="grid grid-cols-2 gap-3">
              <PortField label="Setup script port" hint="Where Prepare another machine serves the Windows setup script." value={draft.script_port} onCommit={(script_port) => commit({ script_port })} />
            </div>
          </div>
        </Card>
      ) : null}

      {draft.share_this_computer ? (
        <Card title="Sharing this computer" description="What Set up on the sharing page uses.">
          <div className="space-y-3">
            {os === "windows" ? (
              <Field label="Memory for Docker (GB)" hint="How much WSL may use. The slider on the sharing page sets the same value. 0 means half of this computer's memory.">
                <TextInput
                  value={String(draft.host_memory_gb)}
                  inputMode="numeric"
                  onChange={(e) => setDraft({ ...draft, host_memory_gb: Number(e.target.value.replace(/\D/g, "")) || 0 })}
                  onBlur={() => commit({ host_memory_gb: draft.host_memory_gb })}
                  className="w-32"
                />
              </Field>
            ) : null}
            <Toggle checked={draft.start_at_login} onChange={(on) => commit({ start_at_login: on })} label="Start dockerNanny at login, hidden in the tray, so this computer is ready after a reboot" />
          </div>
        </Card>
      ) : null}

      <Card title="Pairing" description="Both computers must use the same port: the shared one listens on it, the other connects to it.">
        <div className="grid grid-cols-2 gap-3">
          <PortField label="Pairing port" hint={`Default ${DEFAULT_PAIRING_PORT}.`} value={draft.pairing_port} onCommit={(pairing_port) => commit({ pairing_port })} />
        </div>
      </Card>

      <Card title="Maintenance">
        <div className="space-y-3 text-[13px] text-ink-2">
          <div>
            Everything dockerNanny keeps lives in <span className="mono text-ink">{appHome || "~/.dockernanny"}</span>.
          </div>
          <div className="flex items-center justify-between gap-3">
            <span>Ports on localhost not answering after a crash or a sleep? Drop every bridge and let them come back.</span>
            <Button onClick={() => void resetForwards()} disabled={resetting}>
              {resetting ? <SpinnerIcon /> : null} Restart all bridges
            </Button>
          </div>
          {resetNote ? <div className="text-[12px] text-good">{resetNote}</div> : null}
          <LoopbackMachines onError={setError} />
        </div>
      </Card>
    </div>
  );
}

/** Machine records that point back at this computer, usually left over from
 * trying the sharing role against itself. The rail hides them; here they can
 * be removed without touching anything on disk. */
function LoopbackMachines({ onError }: { onError: (message: string) => void }) {
  const machines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const setMachines = useStore((state) => state.setMachines);
  const hidden = hiddenMachines(machines, computerInfo);
  if (hidden.length === 0) return null;
  const remove = (id: string) => {
    api
      .removeMachine(id)
      .then(setMachines)
      .catch((err) => onError(errorMessage(err)));
  };
  return (
    <div className="space-y-2">
      {hidden.map((machine) => (
        <div key={machine.id} className="flex items-center justify-between gap-3">
          <span>
            <span className="font-medium text-ink">{machine.name}</span> <span className="mono text-ink-3">({machine.user}@{machine.host})</span> points at this computer, so it is not listed as a machine.
          </span>
          <Button onClick={() => remove(machine.id)}>Remove</Button>
        </div>
      ))}
    </div>
  );
}

function RoleRow({ checked, onChange, label, hint }: { checked: boolean; onChange: (on: boolean) => void; label: string; hint: string }) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-xl border border-line bg-surface-2/60 px-3 py-2.5">
      <div>
        <div className="text-[13px] font-medium text-ink">{label}</div>
        <div className="mt-0.5 text-[12px] text-ink-2">{hint}</div>
      </div>
      <div className="pt-0.5">
        <Toggle checked={checked} onChange={onChange} />
      </div>
    </div>
  );
}

/** One line, comma separated; the list is what gets saved. */
function ExcludesField({ value, onCommit }: { value: string[]; onCommit: (excludes: string[]) => void }) {
  const [text, setText] = useState(value.join(", "));
  useEffect(() => setText(value.join(", ")), [value]);
  return (
    <Field label="Paths not synced" hint="Comma separated. Applied to new stacks; .git and node_modules are the usual suspects.">
      <TextInput value={text} onChange={(e) => setText(e.target.value)} onBlur={() => onCommit(text.split(",").map((part) => part.trim()).filter(Boolean))} className="mono" />
    </Field>
  );
}

/** Windows: the WSL distribution the app works through. ssh and rsync run in
 * it for "Use other machines"; Docker and sshd run in it when shared. */
function WslCard({ draft, onCommit }: { draft: Settings; onCommit: (change: Partial<Settings>) => void }) {
  const [installed, setInstalled] = useState<string[] | null>(null);
  useEffect(() => {
    void api
      .wslDistros()
      .then(setInstalled)
      .catch(() => setInstalled([]));
  }, []);
  // The saved choice stays selectable even when WSL does not list it (yet).
  const names = installed && !installed.includes(draft.wsl_distro) ? [draft.wsl_distro, ...installed] : (installed ?? [draft.wsl_distro]);
  const none = installed !== null && installed.length === 0;
  return (
    <Card title="WSL" description="ssh and rsync run inside this Linux distribution. When this computer is shared, Docker and the SSH server run there too.">
      <div className="grid grid-cols-2 gap-3">
        <Field label="Distribution" hint={none ? "WSL lists none. In PowerShell: wsl --install -d Ubuntu" : "Applies at once. When this computer is shared, run Set up again for the new one."}>
          <Select value={draft.wsl_distro} onChange={(e) => onCommit({ wsl_distro: e.target.value })}>
            {names.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </Select>
        </Field>
        {draft.share_this_computer ? <PortField label="SSH port when shared" hint="Run Set up again after changing it." value={draft.wsl_ssh_port} onCommit={(wsl_ssh_port) => onCommit({ wsl_ssh_port })} /> : null}
      </div>
    </Card>
  );
}

/** The backend falls back to the default when the typed name is empty or not a plain image reference. */
function HelperImageField({ value, onCommit }: { value: string; onCommit: (image: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <Field label="Image for copying volumes" hint="A small image with sh and tar. Both ends pull it during a copy; offline machines need it already there. Default alpine:3.">
      <TextInput value={text} onChange={(e) => setText(e.target.value)} onBlur={() => onCommit(text.trim())} className="mono" placeholder="alpine:3" />
    </Field>
  );
}

function PortField({ label, hint, value, onCommit }: { label: string; hint: string; value: number; onCommit: (port: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  return (
    <Field label={label} hint={hint}>
      <TextInput value={text} inputMode="numeric" onChange={(e) => setText(e.target.value.replace(/\D/g, "").slice(0, 5))} onBlur={() => onCommit(Math.min(65535, Number(text) || 0))} className="tabular" />
    </Field>
  );
}
