import { create } from "zustand";

import { api, errorMessage } from "../lib/ipc";
import type { AvailableUpdate, ComputerInfo, CopyProgress, DoctorRow, Fetched, ForwardState, HostOs, HostSnapshot, Machine, MachineStats, OutputLine, Preview, Settings, Stack, StackStatus, Theme, UpdateStatus } from "../lib/types";

export type View = "stacks" | "guide" | "settings" | "computer" | "help";

/** Which container's logs the drawer streams, and where it lives. */
export interface ContainerTarget {
  machineId: string;
  id: string;
  name: string;
}

export type NoticeTone = "error" | "info";
export interface Notice {
  id: number;
  text: string;
  tone: NoticeTone;
}
let nextNoticeId = 1;

/** How the copy sheet was opened: from a stack card (source known), from a
 * machine page (destination known), or from the header (nothing known). */
export interface CopyIntent {
  open: boolean;
  sourceStackId?: string;
  destinationMachineId?: string;
  /** A compose project on this computer, by name, when opened from its row. */
  sourceProject?: string;
  /** Opened from this computer's page to bring a stack back here. */
  toThisComputer?: boolean;
}
const MAX_HOST_LOG_LINES = 600;
const MAX_OUTPUT_LINES = 400;
const MAX_LOG_LINES = 2000;

interface State {
  view: View;
  /** Null until the backend has answered; then never null again. */
  settings: Settings | null;
  /** No settings file yet: the role chooser is shown once. */
  firstRun: boolean;
  /** The welcome, reopened from Help after the first run. */
  welcomeOpen: boolean;
  /** A newer release the app's checks found; installing waits for the user. */
  update: AvailableUpdate | null;
  /** The last check's answer, for Help. */
  updateStatus: UpdateStatus | null;
  /** The share of an update downloaded while it installs. */
  updateProgress: number | null;
  updateOpen: boolean;
  setUpdateStatus: (status: UpdateStatus) => void;
  setUpdateProgress: (fraction: number | null) => void;
  setUpdateOpen: (open: boolean) => void;
  /** The Add machine dialog, opened from the rail or from the welcome. */
  addMachineOpen: boolean;
  /** The machine whose page should open with the remove question showing. */
  askRemoveFor: string | null;
  askRemoveMachine: (id: string) => void;
  /** Which OS this computer runs, for saying what each role needs here. */
  os: HostOs;
  machines: Machine[];
  stats: Record<string, MachineStats>;
  /** Doctor rows keyed by the machine id being checked, filled as events arrive. */
  doctor: Record<string, DoctorRow[]>;
  appHome: string;
  stacks: Stack[];
  statuses: Record<string, StackStatus>;
  forwards: Record<string, ForwardState>;
  output: Record<string, OutputLine[]>;
  /** Streamed `compose logs` for the stack whose drawer is open. */
  logs: OutputLine[];
  logsFor: string | null;
  /** Streamed `docker logs` for one container on a machine, over its context. */
  containerLog: OutputLine[];
  containerLogsFor: ContainerTarget | null;
  /** A file is being dragged over the window. */
  dragging: boolean;
  /** A dropped compose file waiting for the user to confirm. */
  preview: Preview | null;
  dropError: string | null;
  /** The last time a machine fetched the setup script from this computer. */
  scriptFetched: Fetched | null;
  /** The name this computer shows to others, from the backend. */
  computerName: string;
  /** What this computer is and how it is doing; null until probed. */
  computerInfo: ComputerInfo | null;
  /** The machine whose page is open; null shows every stack. */
  selectedMachineId: string | null;
  /** The copy sheet, with what it was opened for. */
  copy: CopyIntent;
  /** Every copy this run has seen, by the card's stack id. */
  copies: Record<string, CopyProgress>;
  /** The copy whose progress panel is open. */
  progressFor: string | null;
  /** The sharing role's view of this computer; null while the role is off. */
  host: HostSnapshot | null;
  hostLog: string[];
  portMapOpen: boolean;
  activityOpen: boolean;
  /** Toast notices for failures the user should see; auto-dismissed. */
  notices: Notice[];

  setView: (view: View) => void;
  setWelcomeOpen: (open: boolean) => void;
  setAddMachineOpen: (open: boolean) => void;
  selectMachine: (id: string | null) => void;
  setCopyOpen: (copy: CopyIntent) => void;
  setCopyProgress: (progress: CopyProgress) => void;
  clearFinishedCopies: () => void;
  openProgress: (stackId: string | null) => void;
  loadHost: () => Promise<void>;
  loadComputerInfo: () => Promise<void>;
  setHost: (host: HostSnapshot | null) => void;
  appendHostLog: (line: string) => void;
  loadSettings: () => Promise<void>;
  saveSettings: (settings: Settings) => Promise<void>;
  setScriptFetched: (fetched: Fetched | null) => void;
  setPortMapOpen: (open: boolean) => void;
  setActivityOpen: (open: boolean) => void;
  pushNotice: (text: string, tone?: NoticeTone) => void;
  dismissNotice: (id: number) => void;
  /** Reads a dropped or chosen compose file and opens the new-stack sheet;
   * says so while Docker reads it, and says why when it cannot. */
  readComposeFile: (path: string) => Promise<void>;
  /** This computer's page, scrolled to its sharing part (the last section). */
  showSharing: () => void;
  load: () => Promise<void>;
  setMachines: (machines: Machine[]) => void;
  setStats: (machineId: string, stats: MachineStats) => void;
  pushDoctorRow: (machineId: string, row: DoctorRow) => void;
  resetDoctor: (machineId: string) => void;
  setStacks: (stacks: Stack[]) => void;
  setStatus: (stackId: string, status: StackStatus) => void;
  setForward: (stackId: string, state: ForwardState) => void;
  appendOutput: (stackId: string, line: OutputLine) => void;
  clearOutput: (stackId: string) => void;
  appendLog: (stackId: string, line: OutputLine) => void;
  clearLogs: () => void;
  openLogs: (stackId: string | null) => void;
  appendContainerLog: (id: string, line: OutputLine) => void;
  openContainerLogs: (target: ContainerTarget | null) => void;
  setDragging: (dragging: boolean) => void;
  setPreview: (preview: Preview | null) => void;
  setDropError: (error: string | null) => void;
}

/** The CSS follows the system unless the root says otherwise. */
function applyTheme(theme: Theme) {
  if (typeof document === "undefined") return;
  if (theme === "system") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = theme;
  }
}

export const useStore = create<State>((set, get) => ({
  view: "stacks",
  settings: null,
  firstRun: false,
  welcomeOpen: false,
  update: null,
  updateStatus: null,
  updateProgress: null,
  updateOpen: false,
  addMachineOpen: false,
  askRemoveFor: null,
  os: "macos",
  machines: [],
  stats: {},
  doctor: {},
  appHome: "",
  stacks: [],
  statuses: {},
  forwards: {},
  output: {},
  logs: [],
  logsFor: null,
  containerLog: [],
  containerLogsFor: null,
  dragging: false,
  preview: null,
  dropError: null,
  scriptFetched: null,
  computerName: "",
  computerInfo: null,
  selectedMachineId: null,
  copy: { open: false },
  copies: {},
  progressFor: null,
  host: null,
  hostLog: [],
  portMapOpen: false,
  activityOpen: false,
  notices: [],

  setView: (view) => set({ view }),
  setWelcomeOpen: (welcomeOpen) => set({ welcomeOpen }),
  setUpdateStatus: (updateStatus) => set({ updateStatus, update: updateStatus.available }),
  setUpdateProgress: (updateProgress) => set({ updateProgress }),
  setUpdateOpen: (updateOpen) => set({ updateOpen }),
  setAddMachineOpen: (addMachineOpen) => set({ addMachineOpen }),
  askRemoveMachine: (id) => set({ askRemoveFor: id, selectedMachineId: id, view: "stacks" }),
  selectMachine: (selectedMachineId) => set({ selectedMachineId, view: "stacks" }),
  setCopyOpen: (copy) => set({ copy }),
  setCopyProgress: (progress) => set((state) => ({ copies: { ...state.copies, [progress.stack_id]: progress } })),
  clearFinishedCopies: () => set((state) => ({ copies: Object.fromEntries(Object.entries(state.copies).filter(([, c]) => !c.finished_ms)) })),
  // One side panel at a time: opening the progress closes the logs, and the other way round.
  // One drawer at a time: opening one closes the others; closing one leaves the rest alone.
  openProgress: (progressFor) => set(progressFor ? { progressFor, logsFor: null, containerLogsFor: null } : { progressFor: null }),
  loadComputerInfo: async () => {
    const computerInfo = await api.computerInfo();
    set({ computerInfo, computerName: computerInfo.name });
  },
  loadHost: async () => {
    const [host, hostLog] = await Promise.all([api.hostSnapshot(), api.hostLog()]);
    set({ host, hostLog });
  },
  setHost: (host) => set({ host }),
  appendHostLog: (line) =>
    set((state) => {
      const lines = [...state.hostLog, line];
      return { hostLog: lines.length > MAX_HOST_LOG_LINES ? lines.slice(lines.length - MAX_HOST_LOG_LINES) : lines };
    }),
  loadSettings: async () => {
    const view = await api.getSettings();
    applyTheme(view.settings.theme);
    set({ settings: view.settings, firstRun: view.first_run, os: view.os });
  },
  saveSettings: async (settings) => {
    const wasSharing = get().settings?.share_this_computer ?? false;
    const saved = await api.saveSettings(settings);
    applyTheme(saved.theme);
    set({ settings: saved, firstRun: false });
    // Said once, when it changes, because a window that hides instead of
    // quitting looks like the app ignored the close button.
    const startedSharing = saved.share_this_computer && !wasSharing;
    if (startedSharing) {
      const tray = get().os === "macos" ? "the menu bar" : "the tray";
      get().pushNotice(`Sharing is on, so closing this window keeps dockerNanny running in ${tray}. Quit from there.`, "info");
    }
  },
  setScriptFetched: (scriptFetched) => set({ scriptFetched }),
  // The port map and the activity list take the same corner; one replaces the other.
  setPortMapOpen: (portMapOpen) => set(portMapOpen ? { portMapOpen, activityOpen: false } : { portMapOpen }),
  setActivityOpen: (activityOpen) => set(activityOpen ? { activityOpen, portMapOpen: false } : { activityOpen }),
  pushNotice: (text, tone = "error") =>
    set((state) => {
      // Collapse a repeat of the same line, so a retry loop cannot flood the corner.
      const without = state.notices.filter((n) => n.text !== text);
      return { notices: [...without, { id: nextNoticeId++, text, tone }].slice(-4) };
    }),
  dismissNotice: (id) => set((state) => ({ notices: state.notices.filter((n) => n.id !== id) })),
  showSharing: () => {
    set({ view: "computer" });
    // After the page has rendered and been scrolled to its top.
    window.setTimeout(() => document.getElementById("sharing")?.scrollIntoView({ behavior: "smooth", block: "start" }), 80);
  },
  readComposeFile: async (path) => {
    const file = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
    const reading = `Reading ${file}…`;
    set({ dropError: null });
    get().pushNotice(reading, "info");
    // The notice goes as soon as there is an answer; the sheet or the error replaces it.
    const dropReading = () => set((state) => ({ notices: state.notices.filter((n) => n.text !== reading) }));
    try {
      const preview = await api.previewCompose(path);
      dropReading();
      set({ preview, dropError: null });
    } catch (err) {
      dropReading();
      const message = errorMessage(err);
      set({ dropError: message });
      get().pushNotice(`Could not read ${file}: ${message}`);
    }
  },
  load: async () => {
    const [machines, stats, appHome, stacks, statuses, forwards, computerName, copyList] = await Promise.all([
      api.listMachines(),
      api.machineStats(),
      api.appHome(),
      api.listStacks(),
      api.stackStatuses(),
      api.forwardStates(),
      api.computerName(),
      api.copyProgress(),
    ]);
    const copies = Object.fromEntries(copyList.map((p) => [p.stack_id, p]));
    set({ machines, stats, appHome, stacks, statuses, forwards, computerName, copies });
  },
  setMachines: (machines) => set({ machines }),
  setStats: (machineId, stats) => set((state) => ({ stats: { ...state.stats, [machineId]: stats } })),
  pushDoctorRow: (machineId, row) =>
    set((state) => {
      const rows = (state.doctor[machineId] ?? []).filter((existing) => existing.key !== row.key);
      return { doctor: { ...state.doctor, [machineId]: [...rows, row] } };
    }),
  resetDoctor: (machineId) => set((state) => ({ doctor: { ...state.doctor, [machineId]: [] } })),
  setStacks: (stacks) => set({ stacks }),
  setStatus: (stackId, status) => set((state) => ({ statuses: { ...state.statuses, [stackId]: status } })),
  setForward: (stackId, forward) => set((state) => ({ forwards: { ...state.forwards, [stackId]: forward } })),
  appendOutput: (stackId, line) =>
    set((state) => {
      const lines = [...(state.output[stackId] ?? []), line];
      const trimmed = lines.length > MAX_OUTPUT_LINES ? lines.slice(lines.length - MAX_OUTPUT_LINES) : lines;
      return { output: { ...state.output, [stackId]: trimmed } };
    }),
  clearOutput: (stackId) => set((state) => ({ output: { ...state.output, [stackId]: [] } })),
  appendLog: (stackId, line) =>
    set((state) => {
      // A late line from a drawer that was already closed is dropped.
      if (state.logsFor !== stackId) return {};
      const lines = [...state.logs, line];
      return { logs: lines.length > MAX_LOG_LINES ? lines.slice(lines.length - MAX_LOG_LINES) : lines };
    }),
  clearLogs: () => set({ logs: [] }),
  openLogs: (stackId) => set(stackId ? { logsFor: stackId, logs: [], progressFor: null, containerLogsFor: null } : { logsFor: null, logs: [] }),
  appendContainerLog: (id, line) =>
    set((state) => {
      // A late line from a container whose drawer already closed is dropped.
      if (state.containerLogsFor?.id !== id) return {};
      const lines = [...state.containerLog, line];
      return { containerLog: lines.length > MAX_LOG_LINES ? lines.slice(lines.length - MAX_LOG_LINES) : lines };
    }),
  openContainerLogs: (containerLogsFor) => set(containerLogsFor ? { containerLogsFor, containerLog: [], logsFor: null, progressFor: null } : { containerLogsFor: null, containerLog: [] }),
  setDragging: (dragging) => set({ dragging }),
  setPreview: (preview) => set({ preview, dropError: null }),
  setDropError: (dropError) => set({ dropError }),
}));

export function onlineCount(machines: Machine[], stats: Record<string, MachineStats>): number {
  return machines.filter((machine) => stats[machine.id]?.online).length;
}

export function isBusy(status: StackStatus | undefined): boolean {
  const phase = status?.phase ?? "idle";
  return phase === "syncing" || phase === "migrating" || phase === "starting" || phase === "stopping";
}
