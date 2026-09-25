import { useState } from "react";

import { api, errorMessage } from "../lib/ipc";
import { useStore } from "../state/store";
import { Dialog, DialogActions } from "../ui/Dialog";
import { SpinnerIcon } from "../ui/icons";
import { Button } from "../ui/primitives";

/** A newer release, what is in it, and what installing does to the running
 * app. Nothing is downloaded until the user says so, because installing
 * restarts the app and its bridges. */
export function UpdateDialog() {
  const update = useStore((state) => state.update);
  const open = useStore((state) => state.updateOpen);
  const setOpen = useStore((state) => state.setUpdateOpen);
  const os = useStore((state) => state.os);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!update) return null;

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      // The app restarts at the end, so success has nothing left to show.
      await api.installUpdate(setProgress);
    } catch (err) {
      setError(errorMessage(err));
      setInstalling(false);
    }
  };
  const percent = progress === null ? null : Math.round(progress * 100);

  return (
    <Dialog open={open} onClose={() => (installing ? undefined : setOpen(false))} eyebrow="Update" title={`dockerNanny ${update.version} is ready`} width={520} closeOnBackdrop={!installing}>
      {update.notes ? <pre className="selectable mt-3 max-h-56 overflow-auto whitespace-pre-wrap rounded-lg border border-line bg-plane/60 px-3 py-2 text-[12px] leading-relaxed text-ink-2">{update.notes}</pre> : null}
      <p className="mt-3 text-[12px] leading-relaxed text-ink-2">
        Installing restarts dockerNanny. Stacks keep running on their machines; their ports on localhost come back a few seconds after the restart, and sharing pauses for the same moment.
        {os === "windows" ? " Windows shows a small progress window while it installs." : ""}
      </p>
      {installing ? (
        <div className="mt-3">
          <div className="text-[12px] text-ink-2">{percent === null ? "Downloading…" : percent < 100 ? `Downloading, ${percent}%` : "Installing…"}</div>
          <div role="progressbar" aria-label="Update download" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? undefined} className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-hairline">
            <div className="h-full rounded-full bg-accent transition-all" style={{ width: `${percent ?? 15}%` }} />
          </div>
        </div>
      ) : null}
      <DialogActions error={error}>
        <Button tone="ghost" onClick={() => setOpen(false)} disabled={installing}>
          Later
        </Button>
        <Button tone="primary" onClick={() => void install()} disabled={installing}>
          {installing ? <SpinnerIcon /> : null} Restart and update
        </Button>
      </DialogActions>
    </Dialog>
  );
}
