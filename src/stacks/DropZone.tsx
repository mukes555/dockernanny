import type { ReactNode } from "react";

import { api, errorMessage } from "../lib/ipc";
import { dropTarget, visibleMachines } from "../lib/machines";
import type { Machine } from "../lib/types";
import { useStore } from "../state/store";
import { FolderIcon, LogoMark, PlusIcon } from "../ui/icons";
import { Button } from "../ui/primitives";
import { usePresence } from "../ui/usePresence";

/** Picks a compose file, the same as dropping it on the window: the drop
 * sheet opens on the machine whose page is open. */
export function useBrowse() {
  const readComposeFile = useStore((state) => state.readComposeFile);
  const pushNotice = useStore((state) => state.pushNotice);
  return async () => {
    const chosen = await api.pickComposeFile().catch((err) => {
      pushNotice(`Could not open the file picker: ${errorMessage(err)}`);
      return null;
    });
    if (chosen) await readComposeFile(chosen);
  };
}

/** The machine a drop runs on, the same one everywhere it is named. */
function useDropTarget(): Machine | undefined {
  const allMachines = useStore((state) => state.machines);
  const computerInfo = useStore((state) => state.computerInfo);
  const stats = useStore((state) => state.stats);
  const selectedId = useStore((state) => state.selectedMachineId);
  return dropTarget(visibleMachines(allMachines, computerInfo), stats, selectedId);
}

/** The empty state: one big target for the first compose file. */
export function DropHero({ extra }: { extra?: ReactNode }) {
  const dropError = useStore((state) => state.dropError);
  const browse = useBrowse();
  const target = useDropTarget();

  return (
    <div className="flex justify-center pt-6">
      <div className="w-full max-w-xl rounded-2xl border-2 border-dashed border-hairline bg-surface/40 px-10 py-12 text-center">
        <LogoMark size={44} className="mx-auto text-accent" />
        <h1 className="mt-5 text-xl font-semibold tracking-tight text-ink">Drop a docker-compose.yml here</h1>
        <p className="mx-auto mt-2 max-w-md text-[13px] leading-relaxed text-ink-2">
          {target ? (
            <>
              Or a project folder that has one. The stack runs on <span className="font-medium text-ink">{target.name}</span> and its ports show up on this
              computer as localhost.
            </>
          ) : (
            <>Or a project folder that has one. Add a machine in the sidebar first, so there is somewhere to run it.</>
          )}
        </p>
        <div className="mt-6 flex justify-center gap-2">
          <Button onClick={() => void browse()} disabled={!target}>
            <FolderIcon /> Browse
          </Button>
          {extra}
        </div>
        {dropError ? <p className="mt-4 text-[12px] text-critical">{dropError}</p> : null}
      </div>
    </div>
  );
}

/** The way to add a stack once some exist; dropping a file anywhere in the
 * window still works, and the button's hint says so. */
export function NewStackButton({ machine }: { machine?: Machine }) {
  const browse = useBrowse();
  const where = machine ? ` to run on ${machine.name}` : "";
  return (
    <Button tone="primary" onClick={() => void browse()} title={`Pick a compose file or project folder${where}, or drop one anywhere in the window`}>
      <PlusIcon /> New stack…
    </Button>
  );
}

/** Why the last dropped or picked file could not be read, until the next one. */
export function DropErrorLine() {
  const dropError = useStore((state) => state.dropError);
  return dropError ? <p className="text-[12px] text-critical">{dropError}</p> : null;
}

/** Covers the window while a file is dragged over it. */
export function DropOverlay() {
  const dragging = useStore((state) => state.dragging);
  const target = useDropTarget();
  const { mounted, state } = usePresence(dragging);
  if (!mounted) return null;
  return (
    <div data-state={state} className="drop-overlay pointer-events-none fixed inset-0 z-30 flex items-center justify-center bg-plane/70 backdrop-blur-sm">
      <div className="drop-card rounded-3xl border-2 border-dashed border-accent bg-surface px-12 py-10 text-center shadow-2xl">
        <LogoMark size={40} className="mx-auto text-accent" />
        <div className="mt-4 text-lg font-semibold text-ink">{target ? `Drop to run on ${target.name}` : "Drop to preview"}</div>
        <div className="mt-1 text-[13px] text-ink-2">A compose file, or a folder that has one</div>
      </div>
    </div>
  );
}
