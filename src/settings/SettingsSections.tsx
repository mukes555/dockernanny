import { useEffect, useState } from "react";

import { api } from "../lib/ipc";
import { hiddenMachines } from "../lib/machines";
import type { Settings, Theme } from "../lib/types";
import { useStore } from "../state/store";
import { Button, Card, ErrorLine, Field, Inset, SaveOnBlurInput, Select, Toggle } from "../ui/primitives";
import { useAction } from "../ui/useAction";

/** Mirrors `pairing::PORT` in the backend. */
const DEFAULT_PAIRING_PORT = 47433;

/** What every section gets from the page: the saved settings, and a way to
 * change some of them (saved at once). Text fields save when left. */
export interface SectionProps {
  settings: Settings;
  commit: (change: Partial<Settings>) => void;
}

const digits = (limit: number) => (typed: string) => typed.replace(/\D/g, "").slice(0, limit);

export function GeneralSection({ settings, commit }: SectionProps) {
  const os = useStore((state) => state.os);
  return (
    <>
      <Card title="What this computer is for" description="Both can be on at the same time.">
        <div className="space-y-3">
          <RoleRow
            checked={settings.use_machines}
            onChange={(on) => commit({ use_machines: on })}
            label="Use other machines"
            hint="Send your compose stacks to other machines and keep using localhost here."
          />
          <RoleRow
            checked={settings.share_this_computer}
            onChange={(on) => commit({ share_this_computer: on })}
            label="Share this computer"
            hint="Let other computers run their stacks here. This computer's Sharing tab sets it up and shows the pairing code."
          />
        </div>
      </Card>
      <Card title="Appearance">
        <Field label="Theme">
          <Select value={settings.theme} onChange={(e) => commit({ theme: e.target.value as Theme })} className="w-56">
            <option value="system">Follow the system</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </Select>
        </Field>
      </Card>
      {os === "windows" ? <WslCard settings={settings} commit={commit} /> : null}
    </>
  );
}

export function MachinesSection({ settings, commit }: SectionProps) {
  const browseKey = async () => {
    const chosen = await api.pickKeyFile().catch(() => null);
    if (chosen) commit({ key_path: chosen });
  };
  return (
    <Card title="New machines and stacks" description="Defaults for new machines and stacks; each one can still differ.">
      <div className="space-y-4">
        <Field label="Private key" hint="Offered when adding a machine. Empty means the first key found in ~/.ssh.">
          <div className="flex gap-2">
            <SaveOnBlurInput value={settings.key_path} onSave={(key_path) => commit({ key_path })} className="mono" placeholder="~/.ssh/id_ed25519" />
            <Button onClick={() => void browseKey()}>Browse</Button>
          </div>
        </Field>
        <Field label="Paths not synced" hint="Comma separated. Applied to new stacks; .git and node_modules are the usual suspects.">
          <SaveOnBlurInput value={settings.excludes.join(", ")} onSave={(text) => commit({ excludes: commaList(text) })} className="mono" />
        </Field>
        <Field
          label="Image for copying volumes"
          hint="A small image with sh and tar. Both ends pull it during a copy; offline machines need it already there. Default alpine:3."
        >
          {/* The backend falls back to the default when the name is empty or not a plain image reference. */}
          <SaveOnBlurInput value={settings.helper_image} onSave={(text) => commit({ helper_image: text.trim() })} className="mono" placeholder="alpine:3" />
        </Field>
      </div>
    </Card>
  );
}

export function SharingSection({ settings, commit }: SectionProps) {
  const os = useStore((state) => state.os);
  return (
    <Card title="Sharing this computer" description="What Set up on this computer's Sharing tab uses.">
      <div className="space-y-4">
        {os === "windows" ? (
          <Field
            label="Memory for Docker (GB)"
            hint="How much WSL may use. The slider on the Sharing tab sets the same value. 0 means half of this computer's memory."
          >
            <SaveOnBlurInput
              value={String(settings.host_memory_gb)}
              clean={digits(3)}
              onSave={(text) => commit({ host_memory_gb: Number(text) || 0 })}
              inputMode="numeric"
              className="w-32"
            />
          </Field>
        ) : null}
        <Toggle
          checked={settings.start_at_login}
          onChange={(on) => commit({ start_at_login: on })}
          label="Start dockerNanny at login, hidden in the tray, so this computer is ready after a reboot"
        />
      </div>
    </Card>
  );
}

export function AdvancedSection({ settings, commit }: SectionProps) {
  const appHome = useStore((state) => state.appHome);
  const setView = useStore((state) => state.setView);
  return (
    <>
      <Card title="Network ports" description="Both computers must use the same pairing port: the shared one listens on it, the other connects to it.">
        <div className="grid grid-cols-2 gap-4">
          <PortField
            label="Pairing port"
            hint={`Default ${DEFAULT_PAIRING_PORT}.`}
            value={settings.pairing_port}
            onSave={(pairing_port) => commit({ pairing_port })}
          />
          {settings.use_machines ? (
            <PortField
              label="Setup script port"
              hint="Where the guide to prepare a machine serves the Windows setup script."
              value={settings.script_port}
              onSave={(script_port) => commit({ script_port })}
            />
          ) : null}
        </div>
      </Card>
      <Card title="Maintenance">
        <div className="space-y-4 text-[13px] text-ink-2">
          <div>
            Everything dockerNanny keeps lives in <span className="mono text-ink">{appHome || "~/.dockernanny"}</span>.
          </div>
          {settings.use_machines ? (
            <div className="flex items-center justify-between gap-4">
              <span>Ports on localhost not answering after a crash or a sleep? The Ports page restarts every bridge.</span>
              <Button onClick={() => setView("ports")} className="shrink-0">
                Open Ports
              </Button>
            </div>
          ) : null}
          <LoopbackMachines />
        </div>
      </Card>
    </>
  );
}

/** Machine records that point back at this computer, usually left over from
 * trying the sharing role against itself. The sidebar hides them; here they
 * can be removed without touching anything on disk. */
function LoopbackMachines() {
  const machines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const removeMachine = useStore((state) => state.removeMachine);
  const removal = useAction("inline");
  // The same two clicks as removing any other machine.
  const [asking, setAsking] = useState<string | null>(null);
  const hidden = hiddenMachines(machines, computerInfo);
  if (hidden.length === 0) return null;
  const remove = (id: string) => {
    setAsking(null);
    void removal.run(() => removeMachine(id));
  };
  return (
    <div className="space-y-2">
      {hidden.map((machine) => (
        <div key={machine.id} className="flex items-center justify-between gap-3">
          <span>
            <span className="font-medium text-ink">{machine.name}</span>{" "}
            <span className="mono text-ink-3">
              ({machine.user}@{machine.host})
            </span>{" "}
            {asking === machine.id
              ? "and its stacks are forgotten here; nothing on disk changes."
              : "points at this computer, so it is not listed as a machine."}
          </span>
          {asking === machine.id ? (
            <span className="flex shrink-0 gap-2">
              <Button tone="ghost" onClick={() => setAsking(null)}>
                Keep
              </Button>
              <Button tone="danger" onClick={() => remove(machine.id)}>
                Remove
              </Button>
            </span>
          ) : (
            <Button onClick={() => setAsking(machine.id)} busy={removal.busy}>
              Remove…
            </Button>
          )}
        </div>
      ))}
      <ErrorLine error={removal.error} />
    </div>
  );
}

function RoleRow({ checked, onChange, label, hint }: { checked: boolean; onChange: (on: boolean) => void; label: string; hint: string }) {
  return (
    <Inset roomy className="flex items-start justify-between gap-4">
      <div>
        <div className="text-[13px] font-medium text-ink">{label}</div>
        <div className="mt-0.5 text-[12px] text-ink-2">{hint}</div>
      </div>
      <div className="pt-0.5">
        <Toggle checked={checked} onChange={onChange} label={label} hideLabel />
      </div>
    </Inset>
  );
}

function commaList(text: string): string[] {
  return text
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

/** Windows: the WSL distribution the app works through. ssh and rsync run in
 * it for "Use other machines"; Docker and sshd run in it when shared. */
function WslCard({ settings, commit }: SectionProps) {
  const [installed, setInstalled] = useState<string[] | null>(null);
  useEffect(() => {
    api
      .wslDistros()
      .then(setInstalled)
      .catch(() => setInstalled([]));
  }, []);
  // The saved choice stays selectable even when WSL does not list it (yet).
  const listed = installed ?? [];
  const names = listed.includes(settings.wsl_distro) ? listed : [settings.wsl_distro, ...listed];
  const none = installed !== null && installed.length === 0;
  return (
    <Card title="WSL" description="ssh and rsync run inside this Linux distribution. When this computer is shared, Docker and the SSH server run there too.">
      <div className="grid grid-cols-2 gap-4">
        <Field
          label="Distribution"
          hint={
            none ? "WSL lists none. In PowerShell: wsl --install -d Ubuntu" : "Applies at once. When this computer is shared, run Set up again for the new one."
          }
        >
          <Select value={settings.wsl_distro} onChange={(e) => commit({ wsl_distro: e.target.value })}>
            {names.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </Select>
        </Field>
        {settings.share_this_computer ? (
          <PortField
            label="SSH port when shared"
            hint="Run Set up again after changing it."
            value={settings.wsl_ssh_port}
            onSave={(wsl_ssh_port) => commit({ wsl_ssh_port })}
          />
        ) : null}
      </div>
    </Card>
  );
}

function PortField({ label, hint, value, onSave }: { label: string; hint: string; value: number; onSave: (port: number) => void }) {
  return (
    <Field label={label} hint={hint}>
      <SaveOnBlurInput
        value={String(value)}
        clean={digits(5)}
        onSave={(text) => onSave(Math.min(65535, Number(text) || 0))}
        inputMode="numeric"
        className="tabular"
      />
    </Field>
  );
}
