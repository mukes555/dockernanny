import { useState, type ReactNode } from "react";

import { Page, Tabs } from "../ui/Page";
import { CodeBlock } from "../ui/primitives";
import { SetupScriptCard } from "./SetupScriptCard";

type Os = "windows" | "macos" | "linux";

const TABS: Array<{ os: Os; label: string }> = [
  { os: "linux", label: "Linux" },
  { os: "macos", label: "macOS" },
  { os: "windows", label: "Windows" },
];

const TAB_KEY = "guide-tab";

/** The tab last used, so a second machine of the same kind opens where the
 * first one did. Storage can be missing or blocked; then it is Linux, the
 * most common machine to run Docker on. */
function rememberedTab(): Os {
  try {
    const saved = window.localStorage.getItem(TAB_KEY);
    const known = TABS.some((tab) => tab.os === saved);
    return known ? (saved as Os) : "linux";
  } catch {
    return "linux";
  }
}

function rememberTab(os: Os) {
  try {
    window.localStorage.setItem(TAB_KEY, os);
  } catch {
    // Only a convenience; the guide works the same without it.
  }
}

/** What another machine needs before it can run stacks for this computer:
 * sshd reachable on the LAN, Docker Engine with compose, rsync, and this
 * computer's public key. dockerNanny never changes the machine by itself;
 * the commands are meant to be run there. */
export function PrepareGuide() {
  const [os, setOsState] = useState<Os>(rememberedTab);
  const setOs = (next: Os) => {
    setOsState(next);
    rememberTab(next);
  };
  return (
    <Page title="Prepare a machine" summary="What another computer needs before it can run stacks for this one" width="max-w-3xl" tabs={<Tabs value={os} onChange={setOs} tabs={TABS.map((tab) => ({ id: tab.os, label: tab.label }))} />}>
      <p className="text-[13px] leading-relaxed text-ink-2">
        A machine needs four things: sshd reachable on your network, Docker Engine with the compose plugin, rsync, and this computer's public key. Container ports never open on the network; dockerNanny reaches them through the ssh connection. The quickest way is dockerNanny on the
        machine with Share this computer turned on: it does all of this and shows a pairing code for Add machine. The steps below are for doing it by hand.
      </p>
      <div>
        {os === "windows" ? <WindowsSteps /> : null}
        {os === "macos" ? <MacSteps /> : null}
        {os === "linux" ? <LinuxSteps /> : null}
      </div>
    </Page>
  );
}

function WindowsSteps() {
  return (
    <>
      <p className="mt-4 text-[13px] leading-relaxed text-ink-2">
        Windows runs Docker inside WSL2 Ubuntu. dockerNanny talks to it over ssh only, so Ubuntu needs sshd on port 2222 reachable from the LAN, and Windows must keep WSL running.
      </p>

      <Step n={1} title="Install WSL2 with Ubuntu" where="PowerShell as Administrator">
        <CodeBlock code={`wsl --install -d Ubuntu`} />
        <Note>Reboot when asked, open Ubuntu once and create your Linux user.</Note>
      </Step>

      <SetupScriptCard />

      <h2 className="mt-8 text-[13px] font-semibold text-ink-2">Or by hand</h2>

      <Step n={2} title="Install Docker Engine inside Ubuntu" where="Ubuntu terminal">
        <CodeBlock
          code={`curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER
printf '[boot]\\nsystemd=true\\n' | sudo tee /etc/wsl.conf`}
        />
        <Note>systemd makes Docker and sshd start with the distro. Docker Desktop for Windows with WSL integration also works, but it needs a signed-in Windows session.</Note>
      </Step>

      <Step n={3} title="Run sshd inside Ubuntu on port 2222" where="Ubuntu terminal">
        <CodeBlock
          code={`sudo apt-get install -y openssh-server rsync
echo 'Port 2222' | sudo tee /etc/ssh/sshd_config.d/dockernanny.conf
sudo systemctl enable --now ssh`}
        />
        <Note>2222 avoids the Windows OpenSSH server if it is ever installed on 22.</Note>
      </Step>

      <Step n={4} title="Mirror the network and keep the VM alive" where={`%USERPROFILE%\\.wslconfig`}>
        <CodeBlock
          code={`[wsl2]
networkingMode=mirrored
vmIdleTimeout=-1
memory=8GB`}
        />
        <Note>
          Mirrored mode (Windows 11 22H2 and newer) makes WSL reachable on the machine's own LAN address. vmIdleTimeout keeps the distro running when no terminal is open. Set memory
          to what Docker may use, about half of the machine's memory. Then run <span className="mono text-ink">wsl --shutdown</span> once.
        </Note>
      </Step>

      <Step n={5} title="Let SSH through the firewall" where="PowerShell as Administrator">
        <CodeBlock
          code={`New-NetFirewallHyperVRule -Name dockerNannySsh -DisplayName "dockerNanny SSH" -Direction Inbound -VMCreatorId '{40E0AC32-46A5-438A-A0B2-2B479E8F2E90}' -Protocol TCP -LocalPorts 2222
New-NetFirewallRule -DisplayName "dockerNanny SSH" -Direction Inbound -Protocol TCP -LocalPort 2222 -Action Allow`}
        />
        <Note>Only port 2222 opens. A network marked Public blocks inbound rules; mark your home or office network Private in Windows settings.</Note>
      </Step>

      <Step n={6} title="Keep the machine awake with the lid closed" where="Windows settings">
        <Note>
          Control Panel, Power Options, "Choose what closing the lid does": set both to Do nothing. Then create a Task Scheduler task that runs at log on with the action{" "}
          <span className="mono text-ink">wsl.exe -d Ubuntu -e true</span>, so the distro (and sshd) starts after a reboot. Windows must be signed in for WSL to run.
        </Note>
      </Step>

      <Step n={7} title="Give this computer access" where="Terminal on this computer">
        <CodeBlock code={`ssh-copy-id -p 2222 <linux-user>@<machine-address>`} />
        <Note>
          Find the machine's address with <span className="mono text-ink">ipconfig</span> in PowerShell. Then click Add machine here, port 2222.
        </Note>
      </Step>
    </>
  );
}

function MacSteps() {
  return (
    <>
      <p className="mt-4 text-[13px] leading-relaxed text-ink-2">A macOS machine needs Docker Desktop or OrbStack, Remote Login, and your key. rsync ships with macOS.</p>

      <Step n={1} title="Install Docker" where="On the machine">
        <Note>
          Install Docker Desktop (docker.com) or OrbStack (orbstack.dev) and open it once. Both provide the <span className="mono text-ink">docker</span> command with the compose
          plugin.
        </Note>
      </Step>

      <Step n={2} title="Turn on Remote Login" where="Terminal on the machine">
        <CodeBlock code={`sudo systemsetup -setremotelogin on`} />
        <Note>Or System Settings, General, Sharing, Remote Login. This is sshd on port 22.</Note>
      </Step>

      <Step n={3} title="Keep it awake" where="System Settings on the machine">
        <Note>Energy (or Battery, Options): turn on "Prevent automatic sleeping when the display is off" and keep the machine plugged in.</Note>
      </Step>

      <Step n={4} title="Give this computer access" where="Terminal on this computer">
        <CodeBlock code={`ssh-copy-id <user>@<machine-address>`} />
        <Note>
          The address is under System Settings, General, Sharing, or <span className="mono text-ink">ipconfig getifaddr en0</span>. Then click Add machine here, port 22.
        </Note>
      </Step>
    </>
  );
}

function LinuxSteps() {
  return (
    <>
      <p className="mt-4 text-[13px] leading-relaxed text-ink-2">Any Linux box works: Docker Engine, sshd, rsync, your key. The commands below are for Debian and Ubuntu; other distributions differ only in the package manager.</p>

      <Step n={1} title="Install Docker Engine" where="Terminal on the machine">
        <CodeBlock
          code={`curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER
sudo systemctl enable --now docker`}
        />
        <Note>Every new ssh login picks up the docker group at once, and Check connection in dockerNanny logs in afresh. For Docker without sudo on the machine's own desktop too, restart it once.</Note>
      </Step>

      <Step n={2} title="Install sshd and rsync" where="Terminal on the machine">
        <CodeBlock
          code={`sudo apt-get install -y openssh-server rsync
sudo systemctl enable --now ssh`}
        />
        <Note>
          With a firewall on, allow port 22: <span className="mono text-ink">sudo ufw allow 22/tcp</span>. Container ports stay closed.
        </Note>
      </Step>

      <Step n={3} title="Give this computer access" where="Terminal on this computer">
        <CodeBlock code={`ssh-copy-id <linux-user>@<machine-address>`} />
        <Note>
          Find the address with <span className="mono text-ink">hostname -I</span> on the machine. Then click Add machine here, port 22.
        </Note>
      </Step>
    </>
  );
}

function Step({ n, title, where, children }: { n: number; title: string; where: string; children: ReactNode }) {
  return (
    <section className="mt-5 rounded-xl border border-line bg-surface p-5">
      <div className="flex items-baseline gap-3">
        <span className="tabular text-[11px] font-semibold text-accent">{String(n).padStart(2, "0")}</span>
        <h2 className="text-[14px] font-semibold text-ink">{title}</h2>
        <span className="ml-auto text-[11px] text-ink-3">{where}</span>
      </div>
      <div className="mt-3 space-y-2">{children}</div>
    </section>
  );
}

function Note({ children }: { children: ReactNode }) {
  return <p className="text-[12px] leading-relaxed text-ink-2">{children}</p>;
}
