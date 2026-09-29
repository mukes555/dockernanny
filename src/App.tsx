import { useEffect, useRef } from "react";

import { ErrorBoundary } from "./app/ErrorBoundary";
import { HelpPage } from "./app/HelpPage";
import { NoticeStack } from "./app/NoticeStack";
import { Sidebar } from "./app/Sidebar";
import { Welcome } from "./app/Welcome";
import { PrepareGuide } from "./guide/PrepareGuide";
import { ComputerPage } from "./computer/ComputerPage";
import { watchFileDrop } from "./lib/dragdrop";
import { api, errorMessage } from "./lib/ipc";
import { SettingsPage } from "./settings/SettingsPage";
import { ActivityPage } from "./stacks/ActivityPage";
import { DropOverlay } from "./stacks/DropZone";
import { DropSheet } from "./stacks/DropSheet";
import { LogDrawer } from "./stacks/LogDrawer";
import { ContainerLogsDrawer } from "./machines/ContainerLogsDrawer";
import { CopyProgressDrawer } from "./stacks/CopyProgress";
import { CopySheet } from "./stacks/CopySheet";
import { PortsPage } from "./stacks/PortsPage";
import { StacksView } from "./stacks/StacksView";
import { useStore } from "./state/store";

export default function App() {
  const view = useStore((state) => state.view);
  const selectedMachineId = useStore((state) => state.selectedMachineId);
  const mainRef = useRef<HTMLElement>(null);

  // The wiring below lives as long as the window. Screens read the store
  // through selectors; this wiring calls its actions, which never change,
  // and reads the latest state when an event arrives.
  useEffect(() => {
    const store = useStore.getState();

    // Nothing should fail into the console alone: an async action that throws
    // where no screen catches it, or a promise nobody awaited, becomes a notice.
    const onError = (event: ErrorEvent) => store.pushNotice(event.message || "Something went wrong in the window.");
    const onRejection = (event: PromiseRejectionEvent) => store.pushNotice(errorMessage(event.reason));
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);

    const unsubscribe = api.subscribe({
      onStats: (event) => store.setStats(event.machine_id, event.stats),
      onDoctor: (event) => store.pushDoctorRow(event.machine_id, event.row),
      onStackStatus: (event) => store.setStatus(event.stack_id, event.status),
      onStackOutput: (event) => store.appendOutput(event.stack_id, event.lines),
      onStackLog: (event) => store.appendLog(event.stack_id, event.lines),
      onForward: (event) => store.setForward(event.stack_id, event.state),
      onScriptFetched: store.setScriptFetched,
      onHostSnapshot: store.setHost,
      onHostLog: (event) => store.appendHostLog(event.line),
      onContainerLog: (event) => store.appendContainerLog(event.id, event.lines),
      onCopyProgress: (progress) => {
        // Notice the failure once, when it lands, so a closed panel does not hide it.
        const before = useStore.getState().copies[progress.stack_id];
        store.setCopyProgress(progress);
        if (progress.failed && progress.finished_ms && !before?.finished_ms) {
          store.pushNotice(`Copy of ${progress.name} to ${progress.to} failed. Open Activity to see why.`);
        }
      },
      onUpdateStatus: store.setUpdateStatus,
      onUpdateProgress: store.setUpdateProgress,
      onOpenUpdate: () => store.openSettings("updates"),
    });

    const started = (what: string, run: () => Promise<unknown>) => void run().catch((err) => store.pushNotice(`Could not load ${what}: ${errorMessage(err)}`));
    started("settings", store.loadSettings);
    started("machines and stacks", store.load);
    started("the sharing role", store.loadHost);
    started("this computer's details", store.loadComputerInfo);
    // The app checks for releases by itself; a check that ended before the
    // window listened is read here.
    started("the update check", () => api.updateStatus().then(store.setUpdateStatus));

    // A drop lands only where it can do something: not over an open dialog
    // (a second sheet would stack on it), and not while using machines is off.
    const accepting = () => {
      const dialogOpen = document.querySelector('[role="dialog"]') !== null;
      const usesMachines = useStore.getState().settings?.use_machines ?? true;
      return usesMachines && !dialogOpen;
    };
    const stopWatchingDrops = watchFileDrop({
      onHover: (hovering) => store.setDragging(hovering && accepting()),
      onDrop: (paths) => {
        const [first] = paths;
        if (!first || !accepting()) return;
        void store.readComposeFile(first);
      },
    });

    return () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
      unsubscribe();
      stopWatchingDrops();
    };
  }, []);

  // A new page starts at its top; the main area is one scroller shared by every page.
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [view, selectedMachineId]);

  // Two boundaries: a broken page keeps the sidebar usable; anything else
  // that breaks (a sheet, a drawer) shows the recovery screen, not a blank window.
  return (
    <ErrorBoundary>
      <div className="flex h-full">
        <Sidebar />
        <main ref={mainRef} className="min-h-0 min-w-0 flex-1 overflow-auto">
          <ErrorBoundary>
            {view === "stacks" ? <StacksView /> : null}
            {view === "ports" ? <PortsPage /> : null}
            {view === "activity" ? <ActivityPage /> : null}
            {view === "computer" ? <ComputerPage /> : null}
            {view === "guide" ? <PrepareGuide /> : null}
            {view === "settings" ? <SettingsPage /> : null}
            {view === "help" ? <HelpPage /> : null}
          </ErrorBoundary>
        </main>
        <Welcome />
        <DropOverlay />
        <DropSheet />
        <CopySheet />
        <NoticeStack />
        <LogDrawer />
        <ContainerLogsDrawer />
        <CopyProgressDrawer />
      </div>
    </ErrorBoundary>
  );
}
