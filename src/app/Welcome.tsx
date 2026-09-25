import { useEffect, useState, type ReactNode } from "react";

import { errorMessage } from "../lib/ipc";
import type { HostOs } from "../lib/types";
import { useStore } from "../state/store";
import { Dialog } from "../ui/Dialog";
import { ArrowLeftIcon, CheckIcon, LogoMark, MachineIcon } from "../ui/icons";
import { Button, cx } from "../ui/primitives";

type Step = "intro" | "roles" | "next";

/** What each role needs on this operating system, in one honest sentence. */
const NEEDS: Record<"use" | "share", Record<HostOs, string>> = {
  use: {
    macos: "Needs an SSH key, which dockerNanny can make for you. ssh and rsync ship with macOS.",
    linux: "Needs an SSH key, which dockerNanny can make for you, plus ssh and rsync from your package manager.",
    windows: "Needs WSL 2 with a Linux distribution; ssh and rsync run inside it.",
  },
  share: {
    macos: "Needs Docker Desktop or OrbStack. dockerNanny turns on Remote Login (ssh) with your password.",
    linux: "Needs Docker Engine and an SSH server. dockerNanny shows the commands for anything missing.",
    windows: "Needs Windows 11 22H2 or newer. dockerNanny installs WSL 2, Ubuntu, Docker and an SSH server, asking first.",
  },
};

/** The first thing a new user sees: what the app is for, which roles this
 * computer plays, and where to go next. Reopenable from Help. */
export function Welcome() {
  const firstRun = useStore((state) => state.firstRun);
  const settings = useStore((state) => state.settings);
  const saveSettings = useStore((state) => state.saveSettings);
  const welcomeOpen = useStore((state) => state.welcomeOpen);
  const setWelcomeOpen = useStore((state) => state.setWelcomeOpen);
  const setAddMachineOpen = useStore((state) => state.setAddMachineOpen);
  const setView = useStore((state) => state.setView);
  const os = useStore((state) => state.os);
  const [step, setStep] = useState<Step>("intro");
  const [useMachines, setUseMachines] = useState(true);
  const [share, setShare] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = (firstRun && settings !== null) || welcomeOpen;
  // Roles must be chosen once; after that the welcome is just a guide.
  const dismissable = !firstRun;

  useEffect(() => {
    if (!open || !settings) return;
    setStep("intro");
    setError(null);
    if (!firstRun) {
      setUseMachines(settings.use_machines);
      setShare(settings.share_this_computer);
    }
    // Only when it opens: later setting changes must not reset the steps.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const saveRoles = async () => {
    if (!settings) return;
    try {
      // Saving ends the first run, which would close the dialog; keep it open for the next step.
      setWelcomeOpen(true);
      await saveSettings({ ...settings, use_machines: useMachines, share_this_computer: share });
      setStep("next");
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  const finish = (then?: () => void) => {
    setWelcomeOpen(false);
    then?.();
  };
  const openSharing = useStore((state) => state.showSharing);

  const titles: Record<Step, string> = { intro: "Welcome to dockerNanny", roles: "What is this computer for?", next: "You are set" };

  return (
    <Dialog open={open} onClose={() => (dismissable ? finish() : undefined)} eyebrow={`Step ${step === "intro" ? 1 : step === "roles" ? 2 : 3} of 3`} title={titles[step]} width={600} closeOnBackdrop={dismissable}>
      {step === "intro" ? (
        <>
          <p className="mt-2 text-[13px] leading-relaxed text-ink-2">
            Run your Docker Compose stacks on another computer on your network and keep using them on <span className="mono text-ink">localhost</span> here, as if they ran on this
            one. Or let this computer run stacks for others.
          </p>
          <Diagram />
          <p className="mt-4 text-[12px] leading-relaxed text-ink-3">
            Everything goes over ssh: dockerNanny copies the project folder with rsync, runs <span className="mono">docker compose</span> there, and carries each published port back to
            this computer. Nothing is opened on the network beyond ssh.
          </p>
          <div className="mt-6 flex justify-end gap-2">
            {dismissable ? (
              <Button tone="ghost" onClick={() => finish()}>
                Close
              </Button>
            ) : null}
            <Button tone="primary" onClick={() => setStep("roles")}>
              Next
            </Button>
          </div>
        </>
      ) : null}

      {step === "roles" ? (
        <>
          <p className="mt-1 text-[13px] text-ink-2">Pick one or both. You can change this any time in Settings.</p>
          <div className="mt-4 grid grid-cols-2 gap-3">
            <RoleCard selected={useMachines} onClick={() => setUseMachines(!useMachines)} icon={<MachineIcon size={20} />} title="Use other machines" text="Send your compose stacks to other machines and keep using localhost here." needs={NEEDS.use[os]} />
            <RoleCard selected={share} onClick={() => setShare(!share)} icon={<LogoMark size={20} />} title="Share this computer" text="Let other computers on your network run their stacks here, after a pairing code." needs={NEEDS.share[os]} />
          </div>
          {error ? <div className="mt-3 text-[12px] text-critical">{error}</div> : null}
          <div className="mt-6 flex items-center justify-between gap-3">
            <Button tone="ghost" onClick={() => setStep("intro")}>
              <ArrowLeftIcon size={13} /> Back
            </Button>
            <div className="flex items-center gap-3">
              <p className="text-[12px] text-ink-3">{!useMachines && !share ? "Pick at least one to continue." : null}</p>
              <Button tone="primary" onClick={() => void saveRoles()} disabled={!useMachines && !share}>
                Continue
              </Button>
            </div>
          </div>
        </>
      ) : null}

      {step === "next" ? (
        <>
          <p className="mt-1 text-[13px] text-ink-2">Here is where to start.</p>
          <div className="mt-4 space-y-2">
            {useMachines ? (
              <NextStep title="Add your first machine" text="Pair with a computer that shows a pairing code, or enter any machine with sshd and Docker by hand." action="Add machine" onClick={() => finish(() => setAddMachineOpen(true))} primary />
            ) : null}
            {useMachines ? <NextStep title="Prepare a machine first" text="Step-by-step instructions for Windows, macOS and Linux machines." action="Open the guide" onClick={() => finish(() => setView("guide"))} /> : null}
            {share ? <NextStep title="Set up sharing" text="Check what this computer has, set up what is missing, and turn on a pairing code." action="Set up" onClick={() => finish(openSharing)} primary={!useMachines} /> : null}
          </div>
          <div className="mt-6 flex justify-end">
            <Button tone="ghost" onClick={() => finish()}>
              I will look around first
            </Button>
          </div>
        </>
      ) : null}
    </Dialog>
  );
}

/** This computer on the left, a machine on the right, ssh between them. */
function Diagram() {
  return (
    <div className="mt-5 flex items-stretch gap-3" aria-hidden>
      <Box title="This computer" lines={["your editor and browser", "localhost:3000"]} />
      <div className="flex flex-col items-center justify-center text-[11px] text-ink-3">
        <span>ssh</span>
        <ArrowLeftIcon size={16} className="rotate-180" />
      </div>
      <Box title="A machine" lines={["Docker runs the stack", "your data stays there"]} />
    </div>
  );
}

function Box({ title, lines }: { title: string; lines: string[] }) {
  return (
    <div className="flex-1 rounded-xl border border-line bg-surface-2/60 px-4 py-3">
      <div className="text-[13px] font-semibold text-ink">{title}</div>
      {lines.map((line) => (
        <div key={line} className="mt-0.5 text-[12px] text-ink-2">
          {line}
        </div>
      ))}
    </div>
  );
}

function RoleCard({ selected, onClick, icon, title, text, needs }: { selected: boolean; onClick: () => void; icon: ReactNode; title: string; text: string; needs: string }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={selected}
      onClick={onClick}
      className={cx("rounded-xl border p-4 text-left transition", selected ? "border-accent bg-accent-soft" : "border-line bg-surface-2/60 hover:border-ink-3")}
    >
      <div className="flex items-center justify-between">
        <span className={cx("flex h-9 w-9 items-center justify-center rounded-lg", selected ? "bg-accent/15 text-accent" : "bg-surface text-ink-3")}>{icon}</span>
        <span className={cx("flex h-5 w-5 items-center justify-center rounded-full border-2 transition", selected ? "border-accent bg-accent text-white" : "border-hairline text-transparent")}>
          <CheckIcon size={12} />
        </span>
      </div>
      <span className="mt-3 block text-[14px] font-semibold text-ink">{title}</span>
      <p className="mt-1 text-[12px] leading-relaxed text-ink-2">{text}</p>
      <p className="mt-2 text-[11px] leading-relaxed text-ink-3">{needs}</p>
    </button>
  );
}

function NextStep({ title, text, action, onClick, primary = false }: { title: string; text: string; action: string; onClick: () => void; primary?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border border-line bg-surface-2/40 px-4 py-3">
      <div>
        <div className="text-[13px] font-medium text-ink">{title}</div>
        <div className="mt-0.5 text-[12px] text-ink-2">{text}</div>
      </div>
      <Button tone={primary ? "primary" : "secondary"} onClick={onClick} className="shrink-0">
        {action}
      </Button>
    </div>
  );
}
