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
  const load = useStore((state) => state.load);
  const loadSettings = useStore((state) => state.loadSettings);
  const setStats = useStore((state) => state.setStats);
  const pushDoctorRow = useStore((state) => state.pushDoctorRow);
  const setStatus = useStore((state) => state.setStatus);
  const setForward = useStore((state) => state.setForward);
  const appendOutput = useStore((state) => state.appendOutput);
  const appendLog = useStore((state) => state.appendLog);
  const setScriptFetched = useStore((state) => state.setScriptFetched);
  const setHost = useStore((state) => state.setHost);
  const appendHostLog = useStore((state) => state.appendHostLog);
  const setCopyProgress = useStore((state) => state.setCopyProgress);
  const appendContainerLog = useStore((state) => state.appendContainerLog);
  const loadHost = useStore((state) => state.loadHost);
  const loadComputerInfo = useStore((state) => state.loadComputerInfo);
  const setDragging = useStore((state) => state.setDragging);
  const pushNotice = useStore((state) => state.pushNotice);

  // Nothing should fail into the console alone: an async action that throws
  // where no screen catches it, or a promise nobody awaited, becomes a notice.
  useEffect(() => {
    const onError = (event: ErrorEvent) => pushNotice(event.message || "Something went wrong in the window.");
    const onRejection = (event: PromiseRejectionEvent) => pushNotice(errorMessage(event.reason));
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);
    return () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    };
  }, [pushNotice]);

  useEffect(() => {
    const unsubscribe = api.subscribe({
      onStats: (event) => setStats(event.machine_id, event.stats),
      onDoctor: (event) => pushDoctorRow(event.machine_id, event.row),
      onStackStatus: (event) => setStatus(event.stack_id, event.status),
      onStackOutput: (event) => appendOutput(event.stack_id, event.lines),
      onStackLog: (event) => appendLog(event.stack_id, event.lines),
      onForward: (event) => setForward(event.stack_id, event.state),
      onScriptFetched: setScriptFetched,
      onHostSnapshot: setHost,
      onHostLog: (event) => appendHostLog(event.line),
      onContainerLog: (event) => appendContainerLog(event.id, event.lines),
      onCopyProgress: (progress) => {
        // Notice the failure once, when it lands, so a closed panel does not hide it.
        const prev = useStore.getState().copies[progress.stack_id];
        setCopyProgress(progress);
        if (progress.failed && progress.finished_ms && !prev?.finished_ms) {
          pushNotice(`Copy of ${progress.name} to ${progress.to} failed. Open Activity to see why.`);
        }
      },
      onUpdateStatus: (status) => useStore.getState().setUpdateStatus(status),
      onUpdateProgress: (fraction) => useStore.getState().setUpdateProgress(fraction),
      onOpenUpdate: () => useStore.getState().openSettings("updates"),
    });
    const started = (what: string, run: () => Promise<unknown>) => void run().catch((err) => pushNotice(`Could not load ${what}: ${errorMessage(err)}`));
    started("settings", loadSettings);
    started("machines and stacks", load);
    started("the sharing role", loadHost);
    started("this computer's details", loadComputerInfo);
    // The app checks for releases by itself; a check that ended before the
    // window listened is read here.
    started("the update check", () => api.updateStatus().then((status) => useStore.getState().setUpdateStatus(status)));
    return unsubscribe;
  }, [load, loadSettings, loadHost, loadComputerInfo, setStats, pushDoctorRow, setStatus, setForward, appendOutput, appendLog, setScriptFetched, setHost, appendHostLog, setCopyProgress, appendContainerLog, pushNotice]);

  // A new page starts at its top; the main area is one scroller shared by every page.
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [view, selectedMachineId]);

  useEffect(() => {
    // A drop lands only where it can do something: not over an open dialog
    // (a second sheet would stack on it), and not while using machines is off.
    const accepting = () => {
      const dialogOpen = document.querySelector('[role="dialog"]') !== null;
      const usesMachines = useStore.getState().settings?.use_machines ?? true;
      return usesMachines && !dialogOpen;
    };
    return watchFileDrop({
      onHover: (hovering) => setDragging(hovering && accepting()),
      onDrop: (paths) => {
        const [first] = paths;
        if (!first || !accepting()) return;
        void useStore.getState().readComposeFile(first);
      },
    });
  }, [setDragging]);

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
