// Mirrors of the Rust structs in src-tauri/src. Field names stay snake_case
// so a payload can be used as it arrives.

export interface Machine {
  id: string;
  name: string;
  user: string;
  host: string;
  port: number;
  key_path: string;
  docker_context: boolean;
  /** The ssh host key was pinned at pairing; connections trust only that key. */
  pinned: boolean;
}

export interface Battery {
  percent: number;
  /** Plugged in, charging or full. */
  charging: boolean;
}

/** What the probe script found on a computer; see probe.rs. */
export interface Probe {
  hostname: string | null;
  os: string | null;
  cpu_model: string | null;
  cpus: number;
  load1: number;
  uptime_s: number;
  mem_used_mb: number;
  mem_total_mb: number;
  disk_free_bytes: number;
  disk_total_bytes: number;
  battery: Battery | null;
  docker_version: string | null;
  flavor: string | null;
  containers_running: number;
}

export interface MachineStats extends Probe {
  online: boolean;
  error: string | null;
}

export interface ComputerInfo {
  name: string;
  user: string;
  probe: Probe;
}

export interface DoctorRow {
  key: string;
  label: string;
  ok: boolean;
  detail: string;
  fix: string | null;
}

export interface DoctorEvent {
  machine_id: string;
  row: DoctorRow;
}

export interface StatsEvent {
  machine_id: string;
  stats: MachineStats;
}

export interface Port {
  target: number;
  published: number;
  protocol: string;
}

export interface ServicePreview {
  name: string;
  image: string | null;
  builds: boolean;
  ports: Port[];
}

export interface Preview {
  project_dir: string;
  compose_rel: string;
  name: string;
  services: ServicePreview[];
  warnings: string[];
  has_env_file: boolean;
  /** Folders inside the project that containers mount; the user picks which are copied. */
  binds: BindMount[];
}

/** A bind mount whose source is inside the project folder, relative to it. */
export interface BindMount {
  path: string;
  read_only: boolean;
  services: string[];
  /** False when only the machine has it (a container wrote it); then it is left alone. */
  exists_here: boolean;
}

export interface Stack {
  id: string;
  name: string;
  machine_id: string;
  project_dir: string;
  compose_rel: string;
  excludes: string[];
  forward_ports: boolean;
  live_sync: boolean;
  /** Published port on the machine to the port used on this computer. Keys are strings once serialised. */
  port_overrides: Record<string, number>;
}

export interface NamedVolume {
  key: string;
  name: string;
  size: string;
  external: boolean;
}

/** A compose project this computer's own Docker knows about. */
export interface LocalProject {
  name: string;
  status: string;
  config_file: string;
  project_dir: string;
  compose_rel: string;
  volumes: NamedVolume[];
  ports: number[];
  warnings: string[];
}

/** One row of a machine's `docker ps -a`, seen through its Docker context. */
export interface Container {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  ports: string;
  created: string;
  project: string | null;
}

export type ContainerAction = "start" | "stop" | "restart";

/** Lines arrive in batches, at most every 100 ms. */
export interface ContainerLogEvent {
  id: string;
  lines: OutputLine[];
}

export type EndpointRef = { kind: "this_computer" } | { kind: "machine"; machine_id: string };

export type CopyStepState = "pending" | "running" | "done" | "failed" | "skipped";

export interface CopyStep {
  name: string;
  state: CopyStepState;
}

/** One stream of bytes on its way; `total_bytes` only for a volume. */
export interface CopyTransfer {
  label: string;
  bytes: number;
  total_bytes: number | null;
  per_second: number;
}

/** Mirrors `copy::progress::CopyProgress`: what a copy is doing, keyed by the card. */
export interface CopyProgress {
  stack_id: string;
  name: string;
  from: string;
  to: string;
  destination: EndpointRef;
  steps: CopyStep[];
  current: CopyTransfer | null;
  lines: string[];
  started_ms: number;
  finished_ms: number | null;
  outcome: string | null;
  failed: boolean;
}

export interface CopyStarted {
  stacks: Stack[];
  card_id: string;
}

/** Mirrors `copy::CopyRequest`. */
export interface CopyRequest {
  source: EndpointRef;
  project?: LocalProject;
  stack_id?: string;
  destination: EndpointRef;
  folder: string;
  name: string;
  config: boolean;
  data: boolean;
  data_selection: DataSelection[];
  stop_source: boolean;
  keep_source_stopped: boolean;
  port_overrides: Record<string, number>;
  excludes: string[];
  forward_ports: boolean;
}

export interface VolumePlan {
  key: string;
  name: string;
  size: string;
  destination_name: string;
  external: boolean;
}

/** What a copy would do, as the backend sees both ends. */
export interface CopyPlan {
  from: string;
  to: string;
  destination_exists: boolean;
  source_running: boolean;
  ports: number[];
  volumes: VolumePlan[];
  containers: ContainerData[];
  images: string[];
  notes: string[];
  warnings: string[];
}

export type Phase = "idle" | "syncing" | "migrating" | "starting" | "running" | "partial" | "stopped" | "stopping" | "error";

export interface ServiceState {
  service: string;
  container: string;
  state: string;
  health: string;
  exit_code: number;
  ports: Port[];
}

export interface StackStatus {
  phase: Phase;
  services: ServiceState[];
  message: string | null;
  synced_at_ms: number | null;
  synced_files: number;
  known: boolean;
  /** The project folder is gone from the machine; Start copies it again. */
  folder_missing?: boolean;
}

export interface OutputLine {
  stream: "stdout" | "stderr";
  text: string;
}

export interface StackStatusEvent {
  stack_id: string;
  status: StackStatus;
}

/** Lines arrive in batches, at most every 100 ms. */
export interface StackOutputEvent {
  stack_id: string;
  lines: OutputLine[];
}

export interface ScriptRequest {
  key_path: string;
  port: number;
  memory_gb: number;
  distro: string;
  /** Change the machine's lid and sleep settings. */
  keep_awake: boolean;
  /** Mark a Public network Private. */
  make_private: boolean;
}

/** What the terminal dialog shows: the config as ssh reads it, and the WSL distribution on Windows. */
export interface TerminalInfo {
  ssh_config: string;
  wsl_distro: string | null;
}

export interface ServeInfo {
  addresses: string[];
  port: number;
}

export interface Fetched {
  from: string;
  at_ms: number;
}

/** One line of `compose logs`, split into who said it and what. */
export interface LogLine {
  container: string;
  text: string;
}

export function parseLogLine(raw: string): LogLine {
  const separator = raw.indexOf(" | ");
  if (separator < 0) return { container: "", text: raw };
  return { container: raw.slice(0, separator).trim(), text: raw.slice(separator + 3) };
}

export interface ForwardPort {
  local: number;
  remote: number;
}

export interface ForwardState {
  up: boolean;
  ports: ForwardPort[];
  error: string | null;
  attempts: number;
  /** When the bridge came up; null while it is down. */
  since_ms: number | null;
}

export interface ForwardEvent {
  stack_id: string;
  state: ForwardState;
}

/** The port a stack's published port answers on, on this computer. */
export function localPort(stack: Stack, published: number): number {
  return stack.port_overrides[String(published)] ?? published;
}

export type Theme = "system" | "dark" | "light";

/** Mirrors `settings::Settings`; see there for what each field means. */
export interface Settings {
  use_machines: boolean;
  share_this_computer: boolean;
  key_path: string;
  excludes: string[];
  theme: Theme;
  host_memory_gb: number;
  start_at_login: boolean;
  pairing_port: number;
  script_port: number;
  /** The image that reads and writes volumes during a copy. */
  helper_image: string;
  /** Windows only: the WSL distribution for ssh and rsync, and for sharing. */
  wsl_distro: string;
  /** Windows only: the sshd port inside WSL when shared. */
  wsl_ssh_port: number;
  /** Look for a newer release at start and twice a day. */
  check_updates: boolean;
}

export type HostOs = "macos" | "windows" | "linux";

/** A newer release found by the updater, not installed yet. */
export interface AvailableUpdate {
  version: string;
  /** The release notes, as written on the GitHub release. */
  notes: string;
}

/** Mirrors `updates::UpdateStatus`: what the app's own checks found. */
export interface UpdateStatus {
  available: AvailableUpdate | null;
  /** When the last check ended, in milliseconds since 1970. */
  checked_ms: number | null;
  /** "up to date", "0.3.3 is available" or why the check failed. */
  result: string | null;
  checking: boolean;
}

export interface SettingsView {
  settings: Settings;
  first_run: boolean;
  os: HostOs;
}

/** A volume the image created for itself; a plain volume copy would miss it. */
export interface AnonymousVolume {
  name: string;
  destination: string;
  size: string;
}

/** A folder the container changed in its own layer. */
export interface ChangedPath {
  path: string;
  entries: number;
  suggested: boolean;
}

export interface ContainerData {
  service: string;
  container: string;
  anonymous_volumes: AnonymousVolume[];
  changed_paths: ChangedPath[];
  note: string | null;
}

/** One container path that travels with the stack. */
export interface DataSelection {
  service: string;
  path: string;
}

/** Mirrors `host::HostSnapshot`: what the sharing role sees on this computer. */
export interface HostRow {
  name: string;
  /** "restart": all installed; restarting the computer once finishes it. */
  state: "ok" | "missing" | "unknown" | "restart";
  detail: string;
}

export interface HostNetwork {
  name: string;
  public: boolean;
}

export interface HostNotice {
  text: string;
  failed: boolean;
}

export interface HostPairing {
  listening: boolean;
  armed: boolean;
  locked: boolean;
  code: string | null;
  remaining_s: number;
  note: string | null;
}

export interface HostSnapshot {
  os: string;
  probed: boolean;
  rows: HostRow[];
  user: string | null;
  ssh_port: number;
  ready_for_pairing: boolean;
  total_memory_gb: number;
  network: HostNetwork | null;
  addresses: string[];
  setup_running: boolean;
  notice: HostNotice | null;
  pairing: HostPairing;
  paired: PairedComputer[];
  connected: ConnectedComputer[];
}

export interface PairedComputer {
  name: string;
  address: string;
  key_type: string;
  paired_at_ms: number;
}

/** A computer with an ssh session open on the sharing port right now. */
export interface ConnectedComputer {
  address: string;
  name: string | null;
  since_ms: number;
}

export interface HostSetupOptions {
  memory_gb: number;
  make_network_private: string | null;
  /** Windows: change the power settings so it stays awake plugged in. */
  keep_awake: boolean;
}

export interface HostLogEvent {
  line: string;
}
