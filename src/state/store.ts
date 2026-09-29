import { create } from "zustand";

import { api, errorMessage } from "../lib/ipc";
import type {
  AvailableUpdate,
  ComputerInfo,
  CopyProgress,
  DoctorRow,
  Fetched,
  ForwardState,
  HostOs,
  HostSnapshot,
  LogEntry,
  Machine,
  MachineStats,
  OutputLine,
  Preview,
  Settings,
  Stack,
  StackStatus,
  Theme,
  UpdateStatus,
} from "../lib/types";
import { parseLogLine } from "../lib/types";

/** The pages the sidebar leads to. A machine's page is "stacks" with a machine selected. */
export type View = "stacks" | "ports" | "activity" | "computer" | "guide" | "settings" | "help";
export type SettingsSection = "general" | "updates" | "machines" | "sharing" | "advanced";
export type ComputerTab = "overview" | "docker" | "sharing";

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
/** The tabs of a machine's page. */
export type MachineTab = "stacks" | "containers" | "details";
/** What a machine's page can open over itself, from the page or from the sidebar's menu. */
export type MachineDialog = "terminal" | "remove";

const MAX_HOST_LOG_LINES = 600;
const MAX_OUTPUT_LINES = 400;
const MAX_LOG_LINES = 2000;

let nextLogSeq = 0;

/** Log lines as the drawers keep them: numbered, and split into container
 * and text once. `compose logs` prefixes every line with its container;
 * `docker logs` of one container does not, so its text is kept whole. */
function logEntries(lines: OutputLine[], prefixed: boolean): LogEntry[] {
  return lines.map((line) => {
    const parsed = prefixed ? parseLogLine(line.text) : { container: "", text: line.text };
    nextLogSeq += 1;
    return { seq: nextLogSeq, stream: line.stream, ...parsed };
  });
}

function lastLogLines(entries: LogEntry[]): LogEntry[] {
  return entries.length > MAX_LOG_LINES ? entries.slice(entries.length - MAX_LOG_LINES) : entries;
}

interface State {
  view: View;
  /** Kept in the store because the sidebar, the tray and other pages open these parts directly. */
  settingsSection: SettingsSection;
  computerTab: ComputerTab;
  /** Null until the backend has answered; then never null again. */
  settings: Settings | null;
  /** No settings file yet: the role chooser is shown once. */
  firstRun: boolean;
  /** The welcome, reopened from Help after the first run. */
  welcomeOpen: boolean;
  /** The last check's answer, for Settings, Updates. A newer release it
   * found is `updateStatus.available` (see `availableUpdate`). */
  updateStatus: UpdateStatus | null;
  /** The share of an update downloaded while it installs. */
  updateProgress: number | null;
  setUpdateStatus: (status: UpdateStatus) => void;
  setUpdateProgress: (fraction: number | null) => void;
  /** The Add machine dialog, opened from the rail or from the welcome. */
  addMachineOpen: boolean;
  /** The open machine page's tab and dialog. They live here, not in the page,
   * so the sidebar's menu can open them directly. */
  machineTab: MachineTab;
  machineDialog: MachineDialog | null;
  /** Machines whose connection check runs right now. */
  checking: Record<string, boolean>;
  setMachineTab: (tab: MachineTab) => void;
  /** Opens the machine's page with that dialog over it; null closes it. */
  openMachineDialog: (machineId: string, dialog: MachineDialog | null) => void;
  /** Runs the connection check; its rows arrive as events and show on the page's Details tab. */
  checkMachine: (machine: Machine) => Promise<void>;
  /** Reads the machine's numbers now instead of at the next poll. */
  refreshMachine: (machine: Machine) => void;
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
  logs: LogEntry[];
  logsFor: string | null;
  /** Streamed `docker logs` for one container on a machine, over its context. */
  containerLog: LogEntry[];
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
  /** Toast notices for failures the user should see; auto-dismissed. */
  notices: Notice[];

  setView: (view: View) => void;
  openSettings: (section: SettingsSection) => void;
  openComputer: (tab: ComputerTab) => void;
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
  /** Changes some settings and saves them; each change builds on the latest
   * settings, even while the one before is still being saved. */
  changeSettings: (change: Partial<Settings>) => Promise<void>;
  setScriptFetched: (fetched: Fetched | null) => void;
  pushNotice: (text: string, tone?: NoticeTone) => void;
  dismissNotice: (id: number) => void;
  /** Reads a dropped or chosen compose file and opens the new-stack sheet;
   * says so while Docker reads it, and says why when it cannot. */
  readComposeFile: (path: string) => Promise<void>;
  /** This computer's page, on its Sharing tab. */
  showSharing: () => void;
  load: () => Promise<void>;
  setMachines: (machines: Machine[]) => void;
  /** The backend removes a machine's stacks with it, so both lists are read back. */
  removeMachine: (id: string) => Promise<void>;
  setStats: (machineId: string, stats: MachineStats) => void;
  pushDoctorRow: (machineId: string, row: DoctorRow) => void;
  resetDoctor: (machineId: string) => void;
  setStacks: (stacks: Stack[]) => void;
  setStatus: (stackId: string, status: StackStatus) => void;
  setForward: (stackId: string, state: ForwardState) => void;
  appendOutput: (stackId: string, lines: OutputLine[]) => void;
  clearOutput: (stackId: string) => void;
  appendLog: (stackId: string, lines: OutputLine[]) => void;
  clearLogs: () => void;
  openLogs: (stackId: string | null) => void;
  appendContainerLog: (id: string, lines: OutputLine[]) => void;
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
  settingsSection: "general",
  computerTab: "overview",
  settings: null,
  firstRun: false,
  welcomeOpen: false,
  updateStatus: null,
  updateProgress: null,
  addMachineOpen: false,
  machineTab: "stacks",
  machineDialog: null,
  checking: {},
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
  notices: [],

  setView: (view) => set({ view }),
  openSettings: (settingsSection) => set({ view: "settings", settingsSection }),
  openComputer: (computerTab) => set({ view: "computer", computerTab }),
  setWelcomeOpen: (welcomeOpen) => set({ welcomeOpen }),
  setUpdateStatus: (updateStatus) => set({ updateStatus }),
  setUpdateProgress: (updateProgress) => set({ updateProgress }),
  setAddMachineOpen: (addMachineOpen) => set({ addMachineOpen }),
  selectMachine: (selectedMachineId) => set({ selectedMachineId, view: "stacks", machineTab: "stacks", machineDialog: null }),
  setMachineTab: (machineTab) => set({ machineTab }),
  openMachineDialog: (machineId, machineDialog) =>
    set((state) => {
      const samePage = state.view === "stacks" && state.selectedMachineId === machineId;
      return { selectedMachineId: machineId, view: "stacks", machineDialog, machineTab: samePage ? state.machineTab : "stacks" };
    }),
  checkMachine: async (machine) => {
    set((state) => ({
      selectedMachineId: machine.id,
      view: "stacks",
      machineTab: "details",
      machineDialog: null,
      checking: { ...state.checking, [machine.id]: true },
      doctor: { ...state.doctor, [machine.id]: [] },
    }));
    try {
      await api.doctor(machine);
    } catch (err) {
      get().pushNotice(`The check of ${machine.name} did not finish: ${errorMessage(err)}`);
    } finally {
      set((state) => ({ checking: { ...state.checking, [machine.id]: false } }));
    }
  },
  refreshMachine: (machine) => {
    api.pollMachine(machine.id).catch((err) => get().pushNotice(`Could not refresh ${machine.name}: ${errorMessage(err)}`));
  },
  setCopyOpen: (copy) => set({ copy }),
  setCopyProgress: (progress) => set((state) => ({ copies: { ...state.copies, [progress.stack_id]: progress } })),
  clearFinishedCopies: () => set((state) => ({ copies: Object.fromEntries(Object.entries(state.copies).filter(([, c]) => !c.finished_ms)) })),
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
  changeSettings: async (change) => {
    const current = get().settings;
    if (!current) return;
    try {
      await get().saveSettings({ ...current, ...change });
    } catch (err) {
      // What the page shows goes back to what is saved.
      await get()
        .loadSettings()
        .catch(() => {});
      throw err;
    }
  },
  saveSettings: async (settings) => {
    const wasSharing = get().settings?.share_this_computer ?? false;
    // Shown at once, so a second change made while this one is saved builds on it.
    set({ settings });
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
  pushNotice: (text, tone = "error") =>
    set((state) => {
      // Collapse a repeat of the same line, so a retry loop cannot flood the corner.
      const without = state.notices.filter((n) => n.text !== text);
      return { notices: [...without, { id: nextNoticeId++, text, tone }].slice(-4) };
    }),
  dismissNotice: (id) => set((state) => ({ notices: state.notices.filter((n) => n.id !== id) })),
  showSharing: () => set({ view: "computer", computerTab: "sharing" }),
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
  removeMachine: async (id) => {
    const machines = await api.removeMachine(id);
    const stacks = await api.listStacks();
    set((state) => ({ machines, stacks, selectedMachineId: state.selectedMachineId === id ? null : state.selectedMachineId, machineDialog: null }));
  },
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
  appendOutput: (stackId, added) =>
    set((state) => {
      const lines = [...(state.output[stackId] ?? []), ...added];
      const trimmed = lines.length > MAX_OUTPUT_LINES ? lines.slice(lines.length - MAX_OUTPUT_LINES) : lines;
      return { output: { ...state.output, [stackId]: trimmed } };
    }),
  clearOutput: (stackId) => set((state) => ({ output: { ...state.output, [stackId]: [] } })),
  appendLog: (stackId, added) =>
    set((state) => {
      // A late line from a drawer that was already closed is dropped.
      if (state.logsFor !== stackId) return {};
      return { logs: lastLogLines([...state.logs, ...logEntries(added, true)]) };
    }),
  clearLogs: () => set({ logs: [] }),
  openLogs: (stackId) => set(stackId ? { logsFor: stackId, logs: [], progressFor: null, containerLogsFor: null } : { logsFor: null, logs: [] }),
  appendContainerLog: (id, added) =>
    set((state) => {
      // A late line from a container whose drawer already closed is dropped.
      if (state.containerLogsFor?.id !== id) return {};
      return { containerLog: lastLogLines([...state.containerLog, ...logEntries(added, false)]) };
    }),
  openContainerLogs: (containerLogsFor) =>
    set(containerLogsFor ? { containerLogsFor, containerLog: [], logsFor: null, progressFor: null } : { containerLogsFor: null, containerLog: [] }),
  setDragging: (dragging) => set({ dragging }),
  setPreview: (preview) => set({ preview, dropError: null }),
  setDropError: (dropError) => set({ dropError }),
}));

export function onlineCount(machines: Machine[], stats: Record<string, MachineStats>): number {
  return machines.filter((machine) => stats[machine.id]?.online).length;
}

/** A newer release the app's checks found; installing waits for the user. A selector. */
export function availableUpdate(state: State): AvailableUpdate | null {
  return state.updateStatus?.available ?? null;
}

/** An operation is running on the stack (sync, up, down, a copy). */
export function isBusy(status: StackStatus | undefined): boolean {
  const phase = status?.phase ?? "idle";
  return phase === "syncing" || phase === "migrating" || phase === "starting" || phase === "stopping";
}

/** Containers are up, ready or not yet: what Stop, Open, the bridge and the counts follow. */
export function isUp(status: StackStatus | undefined): boolean {
  const phase = status?.phase ?? "idle";
  return phase === "running" || phase === "waiting" || phase === "partial";
}
