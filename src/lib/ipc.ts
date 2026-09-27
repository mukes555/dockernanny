// One door to the backend. Inside Tauri it is invoke/listen; in a plain
// browser (pnpm dev) it is the mock, so the UI can be worked on without ssh.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import { mockApi } from "./mock";
import type {
  DoctorEvent,
  DoctorRow,
  Fetched,
  HostLogEvent,
  HostSetupOptions,
  HostSnapshot,
  ForwardEvent,
  ForwardState,
  LocalProject,
  Machine,
  MachineStats,
  ComputerInfo,
  CopyPlan,
  CopyProgress,
  Container,
  ContainerAction,
  ContainerLogEvent,
  CopyRequest,
  CopyStarted,
  Preview,
  ScriptRequest,
  ServeInfo,
  Settings,
  SettingsView,
  Stack,
  StackOutputEvent,
  StackStatus,
  StackStatusEvent,
  StatsEvent,
  TerminalInfo,
  UpdateStatus,
} from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);

/** Tauri rejects commands with a plain string; normalise to a message. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export interface Handlers {
  onStats: (event: StatsEvent) => void;
  onDoctor: (event: DoctorEvent) => void;
  onStackStatus: (event: StackStatusEvent) => void;
  onStackOutput: (event: StackOutputEvent) => void;
  onStackLog: (event: StackOutputEvent) => void;
  onContainerLog: (event: ContainerLogEvent) => void;
  onForward: (event: ForwardEvent) => void;
  onScriptFetched: (event: Fetched) => void;
  onHostSnapshot: (snapshot: HostSnapshot) => void;
  onHostLog: (event: HostLogEvent) => void;
  onCopyProgress: (progress: CopyProgress) => void;
  onUpdateStatus: (status: UpdateStatus) => void;
  /** The share of an update downloaded, null while its size is unknown. */
  onUpdateProgress: (fraction: number | null) => void;
  /** The tray's "Update to …" was clicked. */
  onOpenUpdate: () => void;
}

function unlistenAll(pending: Array<Promise<() => void>>): () => void {
  return () => {
    for (const p of pending) void p.then((unlisten) => unlisten());
  };
}

const tauriApi = {
  listMachines: () => invoke<Machine[]>("list_machines"),
  machineStats: () => invoke<Record<string, MachineStats>>("machine_stats"),
  appHome: () => invoke<string>("app_home"),
  terminalInfo: () => invoke<TerminalInfo>("terminal_info"),
  wslDistros: () => invoke<string[]>("wsl_distros"),
  computerName: () => invoke<string>("computer_name"),
  computerInfo: () => invoke<ComputerInfo>("computer_info"),
  newMachineId: () => invoke<string>("new_machine_id"),
  defaultKeyPath: () => invoke<string>("default_key_path"),
  computerReadiness: () => invoke<DoctorRow[]>("computer_readiness"),
  generateKey: () => invoke<string>("generate_key"),
  installWslTools: () => invoke<void>("install_wsl_tools"),
  diagnostics: () => invoke<string>("diagnostics"),
  revealAppFile: (which: "folder" | "log") => invoke<void>("reveal_app_file", { which }),
  openLink: (url: string) => openUrl(url),
  // The app checks by itself (see updates.rs); these read it, ask now, and install.
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  checkForUpdate: () => invoke<UpdateStatus>("check_for_update"),
  /** Downloads, verifies and installs what the last check found, then the
   * app restarts; progress arrives as `onUpdateProgress`. */
  installUpdate: () => invoke<void>("install_update"),
  doctor: (machine: Machine) => invoke<DoctorRow[]>("doctor", { machine }),
  addMachine: (machine: Machine) => invoke<Machine[]>("add_machine", { machine }),
  removeMachine: (id: string) => invoke<Machine[]>("remove_machine", { id }),
  pollMachine: (id: string) => invoke<MachineStats>("poll_machine", { id }),
  setDockerContext: (id: string, enabled: boolean) => invoke<Machine[]>("set_docker_context", { id, enabled }),
  listContainers: (machineId: string) => invoke<Container[]>("list_containers", { machineId }),
  containerAction: (machineId: string, id: string, action: ContainerAction) => invoke<void>("container_action", { machineId, id, action }),
  startContainerLogs: (machineId: string, id: string) => invoke<void>("start_container_logs", { machineId, id }),
  stopContainerLogs: (id: string) => invoke<void>("stop_container_logs", { id }),
  pairMachine: (address: string, code: string, keyPath: string, name: string) => invoke<Machine>("pair_machine", { address, code, keyPath, name }),

  previewCompose: (path: string) => invoke<Preview>("preview_compose", { path }),
  defaultExcludes: () => invoke<string[]>("default_excludes"),
  busyPorts: (ports: number[]) => invoke<number[]>("busy_ports", { ports }),
  listStacks: () => invoke<Stack[]>("list_stacks"),
  stackStatuses: () => invoke<Record<string, StackStatus>>("stack_statuses"),
  createStack: (stack: Stack) => invoke<Stack[]>("create_stack", { stack }),
  /** Start syncs and runs `up -d`; `rebuild` adds `--build`. */
  upStack: (id: string, rebuild: boolean) => invoke<void>("up_stack", { id, rebuild }),
  downStack: (id: string) => invoke<void>("down_stack", { id }),
  restartStack: (id: string) => invoke<void>("restart_stack", { id }),
  removeStack: (id: string, volumes: boolean) => invoke<Stack[]>("remove_stack", { id, volumes }),
  forwardStates: () => invoke<Record<string, ForwardState>>("forward_states"),
  setForwardPorts: (id: string, on: boolean) => invoke<Stack[]>("set_forward_ports", { id, on }),
  localProjects: () => invoke<LocalProject[]>("local_projects"),
  copyPlan: (request: CopyRequest) => invoke<CopyPlan>("copy_plan", { request }),
  copyStack: (request: CopyRequest) => invoke<CopyStarted>("copy_stack", { request }),
  copyProgress: () => invoke<CopyProgress[]>("copy_progress"),
  syncStack: (id: string) => invoke<void>("sync_stack", { id }),
  startLogs: (id: string) => invoke<void>("start_logs", { id }),
  stopLogs: (id: string) => invoke<void>("stop_logs", { id }),

  scriptPreview: (request: ScriptRequest) => invoke<string>("script_preview", { request }),
  scriptServe: (request: ScriptRequest) => invoke<ServeInfo>("script_serve", { request }),
  scriptStop: () => invoke<void>("script_stop"),

  getSettings: () => invoke<SettingsView>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  resetForwards: () => invoke<void>("reset_forwards"),

  hostSnapshot: () => invoke<HostSnapshot | null>("host_snapshot"),
  hostLog: () => invoke<string[]>("host_log"),
  hostSetup: (options: HostSetupOptions) => invoke<void>("host_setup", { options }),
  hostArmPairing: () => invoke<void>("host_arm_pairing"),
  hostDisarmPairing: () => invoke<void>("host_disarm_pairing"),
  hostProbe: () => invoke<void>("host_probe"),

  copyText: (text: string) => writeText(text),
  openLocal: (port: number) => openUrl(`http://localhost:${port}`),
  pickKeyFile: async (): Promise<string | null> => {
    const chosen = await open({ multiple: false, directory: false, title: "Choose the private key" });
    return typeof chosen === "string" ? chosen : null;
  },
  pickComposeFile: async (): Promise<string | null> => {
    const chosen = await open({ multiple: false, directory: false, title: "Choose a compose file", filters: [{ name: "Compose file", extensions: ["yml", "yaml"] }] });
    return typeof chosen === "string" ? chosen : null;
  },
  subscribe(handlers: Handlers): () => void {
    return unlistenAll([
      listen<StatsEvent>("machine:stats", (event) => handlers.onStats(event.payload)),
      listen<DoctorEvent>("machine:doctor", (event) => handlers.onDoctor(event.payload)),
      listen<StackStatusEvent>("stack:status", (event) => handlers.onStackStatus(event.payload)),
      listen<StackOutputEvent>("stack:output", (event) => handlers.onStackOutput(event.payload)),
      listen<StackOutputEvent>("stack:log", (event) => handlers.onStackLog(event.payload)),
      listen<ContainerLogEvent>("container:log", (event) => handlers.onContainerLog(event.payload)),
      listen<ForwardEvent>("forward:state", (event) => handlers.onForward(event.payload)),
      listen<Fetched>("guide:fetched", (event) => handlers.onScriptFetched(event.payload)),
      listen<HostSnapshot>("host:snapshot", (event) => handlers.onHostSnapshot(event.payload)),
      listen<HostLogEvent>("host:log", (event) => handlers.onHostLog(event.payload)),
      listen<CopyProgress>("copy:progress", (event) => handlers.onCopyProgress(event.payload)),
      listen<UpdateStatus>("update:status", (event) => handlers.onUpdateStatus(event.payload)),
      listen<number | null>("update:progress", (event) => handlers.onUpdateProgress(event.payload)),
      listen("update:open", () => handlers.onOpenUpdate()),
    ]);
  },
};

export type Api = typeof tauriApi;
export const api: Api = isTauri ? tauriApi : mockApi;
