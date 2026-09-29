import type { ReactNode } from "react";

import type { HostOs } from "../lib/types";

/** Everything Set up may change on this computer, in the order it happens.
 * It must match the steps in the backend (`host/windows_steps.rs` with
 * `host/wsl_setup.sh`, `host/macos.rs`, `host/linux.rs`). Every step looks
 * first and changes only what is missing, so running Set up again changes nothing. */
const CHANGES: Record<HostOs, string[]> = {
  windows: [
    "Turns on WSL 2 (or updates a WSL older than 2.0) and installs the Linux distribution chosen in Settings if it is missing. Windows asks for administrator permission.",
    "Creates a Linux user named nanny when the distribution has none, and makes it the default.",
    "Inside the distribution, as root and only where missing: installs openssh-server, rsync and Docker Engine (with Docker's own script, never over an existing Docker), lets that user run Docker, and sets the SSH port. In /etc/wsl.conf it sets systemd on and the default user, keeping your other lines.",
    "In .wslconfig in your user folder, adds only the lines it needs (mirrored networking, no idle shutdown) and keeps yours. Memory is set only when you chose it above, or when the file has no memory line yet. Before the first change your file is kept as .wslconfig.before-dockernanny.",
    "Adds two firewall rules, for the SSH port and the pairing port, on private and domain networks only. Administrator permission again.",
    "Restarts WSL only when a setting needs it: the whole of WSL when .wslconfig changed (which also stops other WSL distributions, such as Docker Desktop's, for a moment), just the distribution when /etc/wsl.conf changed, otherwise nothing.",
  ],
  macos: ["Turns on Remote Login (the SSH server). macOS asks for your password.", "Creates ~/.ssh/authorized_keys if it is missing."],
  linux: [
    "Only what is missing, as one root script: installs rsync and openssh-server and turns the SSH server on; installs Docker Engine with Docker's official script and starts it (also at boot). The system asks for your password (pkexec), or shows the commands to run yourself.",
    "Adds you to the docker group if you are not in it. As Docker's own guide warns, that lets your user control Docker with root-level power. Your login picks it up after one restart.",
    "Docker installed another way (snap, Docker Desktop, rootless, Podman) is left as it is.",
    "Creates ~/.ssh/authorized_keys if it is missing.",
  ],
};

/** The list, plus the two changes someone may not want on their own
 * computer as choices: power settings, and a network's category. */
export function SetupChanges({
  os,
  open,
  keepAwake,
  onKeepAwake,
  publicNetwork,
  makePrivate,
  onMakePrivate,
}: {
  os: HostOs;
  open: boolean;
  keepAwake: boolean;
  onKeepAwake: (on: boolean) => void;
  publicNetwork: string | null;
  makePrivate: boolean;
  onMakePrivate: (on: boolean) => void;
}) {
  return (
    <details open={open} className="mt-4 rounded-xl border border-line bg-surface-2/40 px-3 py-2.5 text-[12px] text-ink-2">
      <summary className="cursor-pointer text-[13px] font-medium text-ink">What Set up changes on this computer</summary>
      <ol className="mt-2 list-decimal space-y-1 pl-5 leading-relaxed">
        {CHANGES[os].map((change) => (
          <li key={change}>{change}</li>
        ))}
        <li>Later, pairing adds the other computer's public key to that user's authorized_keys. Nothing else is opened on the network.</li>
      </ol>
      {os === "windows" ? (
        <div className="mt-3 space-y-2 border-t border-line pt-3">
          <Choice checked={keepAwake} onChange={onKeepAwake}>
            Keep this computer awake while plugged in, also with the lid closed (power settings). Without it, other computers cannot reach it while it sleeps.
          </Choice>
          {publicNetwork ? (
            <Choice checked={makePrivate} onChange={onMakePrivate}>
              Mark the network "{publicNetwork}" as Private. Windows blocks incoming connections on Public networks, so other computers cannot reach this one
              until then. Only on a network you trust.
            </Choice>
          ) : null}
        </div>
      ) : null}
    </details>
  );
}

function Choice({ checked, onChange, children }: { checked: boolean; onChange: (on: boolean) => void; children: ReactNode }) {
  return (
    <label className="flex items-start gap-2 text-[12px] text-ink-2">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="mt-0.5 accent-accent" />
      <span>{children}</span>
    </label>
  );
}
