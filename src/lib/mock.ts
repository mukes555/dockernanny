// A pretend backend for `pnpm dev` in a browser: fixed machines, a doctor that
// reports rows one by one, stats that wobble, a stack that pretends to build.
// Every name, address and path here is made up (RFC 5737 addresses).

import type { Api, Handlers } from "./ipc";
import type {
  CopyProgress,
  DoctorRow,
  ForwardState,
  HostOs,
  HostSnapshot,
  Machine,
  MachineStats,
  PairedComputer,
  Preview,
  ServiceState,
  Settings,
  Stack,
  StackStatus,
  UpdateStatus,
} from "./types";

const KEY_PATH = "/home/alex/.ssh/id_ed25519";
// The mock starts without a key, so the readiness list shows how one is created.
let keyMade = false;
let wslToolsInstalled = false;

// Null until Continue is clicked in the role chooser, so a reload shows the first launch again.
let settings: Settings | null = null;
const defaultSettings = (): Settings => ({
  use_machines: true,
  share_this_computer: false,
  key_path: "",
  excludes: [".git", "node_modules", ".DS_Store", ".tmp"],
  theme: "system",
  host_memory_gb: 0,
  start_at_login: false,
  pairing_port: 47433,
  script_port: 47431,
  helper_image: "alpine:3",
  wsl_distro: "Ubuntu",
  wsl_ssh_port: 2222,
  check_updates: true,
});

// `?os=windows` or `?os=linux` in the address shows the window as it looks there.
const MOCK_OS: HostOs = ((): HostOs => {
  const asked = new URLSearchParams(window.location.search).get("os");
  return asked === "windows" || asked === "linux" ? asked : "macos";
})();

const mockUpdateStatus = (): UpdateStatus => {
  const offered = new URLSearchParams(window.location.search).get("update") === "1";
  const available = offered ? { version: "0.3.3", notes: "What is new in 0.3.3:\n- A made-up fix, to show the notes.\n- Another one." } : null;
  return { available, checked_ms: Date.now(), result: available ? "0.3.3 is available" : "up to date", checking: false };
};

// `?restart=1`: Docker is installed and only a restart of the computer is left.
const MOCK_RESTART = new URLSearchParams(window.location.search).get("restart") === "1";

// A full online reading with a few fields swapped: the shape the backend sends.
const onlineStats = (over: Partial<MachineStats>): MachineStats => ({
  online: true,
  hostname: "studio",
  os: "Ubuntu 24.04 LTS",
  cpu_model: "Example 8-Core Processor",
  cpus: 8,
  load1: 0.4,
  uptime_s: 2 * 86400 + 3 * 3600,
  mem_used_mb: 2100,
  mem_total_mb: 16000,
  disk_free_bytes: 120e9,
  disk_total_bytes: 500e9,
  battery: null,
  docker_version: "29.0.0",
  flavor: "Ubuntu 24.04 LTS",
  containers_running: 0,
  error: null,
  ...over,
});

const machines: Machine[] = [
  { id: "a1b2c3d4", name: "Studio", user: "alex", host: "192.0.2.10", port: 2222, key_path: KEY_PATH, docker_context: false, pinned: true },
  { id: "e5f6a7b8", name: "Workshop", user: "alex", host: "workshop.local", port: 22, key_path: KEY_PATH, docker_context: true, pinned: false },
  // A leftover from testing the sharing role against itself: hidden from the rail, listed in Settings.
  { id: "00000000", name: "Old test", user: "alex", host: "localhost", port: 22, key_path: KEY_PATH, docker_context: false, pinned: false },
];

const stats: Record<string, MachineStats> = {
  a1b2c3d4: {
    online: true,
    hostname: "studio",
    os: "Windows 11 (build 22631) · Ubuntu 24.04 LTS in WSL2",
    cpu_model: "Example 16-Core Processor",
    cpus: 16,
    load1: 3.4,
    uptime_s: 9 * 86400 + 8 * 3600,
    mem_used_mb: 9800,
    mem_total_mb: 32000,
    disk_free_bytes: 900e9,
    disk_total_bytes: 1000e9,
    battery: { percent: 80, charging: true },
    docker_version: "29.0.0",
    flavor: "Ubuntu 24.04 LTS",
    containers_running: 3,
    error: null,
  },
  e5f6a7b8: {
    online: false,
    hostname: null,
    os: null,
    cpu_model: null,
    cpus: 0,
    load1: 0,
    uptime_s: 0,
    mem_used_mb: 0,
    mem_total_mb: 0,
    disk_free_bytes: 0,
    disk_total_bytes: 0,
    battery: null,
    docker_version: null,
    flavor: null,
    containers_running: 0,
    error: "Nothing answers on port 22.",
  },
};

const stacks: Stack[] = [
  {
    id: "s1s1s1s1",
    name: "shop-api",
    machine_id: "a1b2c3d4",
    project_dir: "/home/alex/projects/shop-api",
    compose_rel: "docker-compose.yml",
    excludes: [".git", "node_modules", ".DS_Store", ".tmp"],
    forward_ports: true,
    live_sync: true,
    port_overrides: { "5432": 6432 },
  },
];

const statuses: Record<string, StackStatus> = {
  s1s1s1s1: {
    phase: "partial",
    known: true,
    error: null,
    sync_warning: null,
    synced_at_ms: Date.now() - 42000,
    synced_files: 142,
    services: [
      {
        service: "api",
        container: "shop-api-api-1",
        state: "running",
        health: "healthy",
        exit_code: 0,
        ports: [{ target: 3000, published: 3000, protocol: "tcp" }],
        job: false,
        readiness: "ready",
      },
      {
        service: "db",
        container: "shop-api-db-1",
        state: "running",
        health: "",
        exit_code: 0,
        ports: [{ target: 5432, published: 5432, protocol: "tcp" }],
        job: false,
        readiness: "ready",
      },
      { service: "migrate", container: "shop-api-migrate-1", state: "exited", health: "", exit_code: 0, ports: [], job: true, readiness: "done" },
      { service: "worker", container: "shop-api-worker-1", state: "exited", health: "", exit_code: 1, ports: [], job: false, readiness: "stopped" },
    ],
  },
};

const forwards: Record<string, ForwardState> = {
  s1s1s1s1: {
    up: true,
    error: null,
    attempts: 0,
    since_ms: Date.now() - 47 * 60_000,
    ports: [
      { local: 3000, remote: 3000 },
      { local: 6432, remote: 5432 },
    ],
  },
};

const samplePreview: Preview = {
  project_dir: "/home/alex/projects/sample-stack",
  compose_rel: "docker-compose.yml",
  name: "sample-stack",
  has_env_file: true,
  services: [
    { name: "echo", image: "hashicorp/http-echo", builds: false, ports: [{ target: 5678, published: 8088, protocol: "tcp" }] },
    { name: "web", image: "nginx:alpine", builds: false, ports: [{ target: 80, published: 8087, protocol: "tcp" }] },
  ],
  warnings: [
    "echo: port 9099/udp is not forwarded (SSH forwards TCP only)",
    "echo: mounts /etc/hosts, which is outside the project folder and will not exist on the machine",
  ],
  binds: [
    { path: "html", read_only: false, services: ["web"], exists_here: true },
    { path: "pgdata", read_only: false, services: ["db"], exists_here: false },
    { path: "conf/nginx.conf", read_only: true, services: ["web"], exists_here: true },
  ],
};

let handlers: Handlers | null = null;
const copies: Record<string, CopyProgress> = {};

/** A copy that takes a few seconds: every step in turn, bytes on the volume. */
function pretendCopy(cardId: string, name: string, from: string, to: string, destination: CopyProgress["destination"], withData: boolean) {
  const names = [
    "looking at both ends",
    ...(withData ? [`stopping ${name} on ${to}`] : []),
    `copying the project folder to ${to}`,
    ...(withData ? ["copying volume pgdata (412MB)", `creating the containers on ${to}`, "copying /var/lib/postgresql/data from db"] : []),
    `starting ${name} on ${to}`,
    `checking the result on ${to}`,
  ];
  const progress: CopyProgress = {
    stack_id: cardId,
    name,
    from,
    to,
    destination,
    steps: names.map((n) => ({ name: n, state: "pending" })),
    current: null,
    lines: [],
    started_ms: Date.now(),
    finished_ms: null,
    outcome: null,
    failed: false,
  };
  const send = () => {
    copies[cardId] = {
      ...progress,
      steps: progress.steps.map((s) => ({ ...s })),
      lines: [...progress.lines],
      current: progress.current ? { ...progress.current } : null,
    };
    handlers?.onCopyProgress(copies[cardId]);
  };
  const say = (text: string) => {
    progress.lines = [...progress.lines, text].slice(-12);
  };
  let index = -1;
  const next = () => {
    if (index >= 0) progress.steps[index].state = "done";
    index += 1;
    if (index >= progress.steps.length) {
      progress.finished_ms = Date.now();
      progress.outcome = `2 of 2 services up, 2 of 2 ports listening on ${to}`;
      say(`    ${progress.outcome}`);
      send();
      return;
    }
    const step = progress.steps[index];
    step.state = "running";
    say(`==> ${step.name}`);
    if (step.name.startsWith("copying volume")) {
      progress.current = { label: "volume pgdata", bytes: 0, total_bytes: 412_000_000, per_second: 0 };
      const ticker = window.setInterval(() => {
        if (!progress.current) return;
        progress.current.bytes = Math.min(412_000_000, progress.current.bytes + 24_000_000);
        progress.current.per_second = 118_000_000;
        send();
        if (progress.current.bytes >= 412_000_000) {
          window.clearInterval(ticker);
          progress.current = null;
          say("    copied, 131 MB through the pipe");
          window.setTimeout(next, 300);
        }
      }, 200);
      send();
      return;
    }
    if (step.name.startsWith("copying the project folder")) say(">f+++++++++ docker-compose.yml");
    send();
    window.setTimeout(next, 900);
  };
  next();
}
let logTimer: number | null = null;
let ctrLogTimer: number | null = null;

// A pretend `docker ps -a` for the online machine, mutated by the actions.
const mockContainers: Record<
  string,
  Array<{ id: string; name: string; image: string; state: string; status: string; ports: string; created: string; project: string | null }>
> = {
  a1b2c3d4: [
    {
      id: "a1b2c3d4e5f6",
      name: "shop-api-api-1",
      image: "shop-api-api:latest",
      state: "running",
      status: "Up 6 minutes",
      ports: "0.0.0.0:3000->3000/tcp",
      created: "2026-01-01 10:00:00",
      project: "shop-api",
    },
    {
      id: "b2c3d4e5f6a7",
      name: "shop-api-db-1",
      image: "postgres:16",
      state: "running",
      status: "Up 6 minutes (healthy)",
      ports: "0.0.0.0:5432->5432/tcp",
      created: "2026-01-01 10:00:00",
      project: "shop-api",
    },
    {
      id: "c3d4e5f6a7b8",
      name: "blog-web-1",
      image: "nginx:alpine",
      state: "running",
      status: "Up 2 hours",
      ports: "0.0.0.0:8081->80/tcp",
      created: "2026-01-01 08:00:00",
      project: "blog",
    },
    {
      id: "d4e5f6a7b8c9",
      name: "shop-api-worker-1",
      image: "shop-api-worker:latest",
      state: "exited",
      status: "Exited (1) 3 minutes ago",
      ports: "",
      created: "2026-01-01 10:00:00",
      project: "shop-api",
    },
    {
      id: "e5f6a7b8c9d0",
      name: "scratch-postgres",
      image: "postgres:16",
      state: "exited",
      status: "Exited (0) 12 days ago",
      ports: "",
      created: "2025-12-20 09:00:00",
      project: null,
    },
    {
      id: "f6a7b8c9d0e1",
      name: "buildx_buildkit_builder0",
      image: "moby/buildkit:buildx-stable-1",
      state: "exited",
      status: "Exited (0) 2 days ago",
      ports: "",
      created: "2025-12-30 09:00:00",
      project: null,
    },
  ],
};
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function publish(stackId: string, change: Partial<StackStatus>) {
  const base: StackStatus = statuses[stackId] ?? {
    phase: "idle",
    services: [],
    error: null,
    sync_warning: null,
    synced_at_ms: null,
    synced_files: 0,
    known: false,
  };
  statuses[stackId] = { ...base, ...change };
  handlers?.onStackStatus({ stack_id: stackId, status: statuses[stackId] });
}

async function pretendUp(stack: Stack) {
  publish(stack.id, { phase: "syncing", error: null });
  for (const file of ["docker-compose.yml", "html/index.html", ".env"]) {
    await wait(250);
    handlers?.onStackOutput({ stack_id: stack.id, lines: [{ stream: "stdout", text: `>f+++++++++ ${file}` }] });
  }
  publish(stack.id, { phase: "starting", synced_at_ms: Date.now(), synced_files: 3 });
  for (const line of [
    " Network sample-stack_default Creating",
    " Container sample-stack-web-1 Starting",
    " Container sample-stack-echo-1 Started",
    " Container sample-stack-web-1 Started",
  ]) {
    await wait(400);
    handlers?.onStackOutput({ stack_id: stack.id, lines: [{ stream: "stderr", text: line }] });
  }
  // Up first, then ready once web's health check passes, as Compose reports it.
  const services = (webHealth: string): ServiceState[] => [
    {
      service: "echo",
      container: `${stack.name}-echo-1`,
      state: "running",
      health: "",
      exit_code: 0,
      ports: [{ target: 5678, published: 8088, protocol: "tcp" }],
      job: false,
      readiness: "ready",
    },
    {
      service: "web",
      container: `${stack.name}-web-1`,
      state: "running",
      health: webHealth,
      exit_code: 0,
      ports: [{ target: 80, published: 8087, protocol: "tcp" }],
      job: false,
      readiness: webHealth === "healthy" ? "ready" : "starting",
    },
  ];
  publish(stack.id, { phase: "waiting", known: true, services: services("starting") });
  await wait(1500);
  publish(stack.id, { phase: "running", services: services("healthy") });
  await wait(600);
  const local = (published: number) => stack.port_overrides[String(published)] ?? published;
  forwards[stack.id] = {
    up: true,
    error: null,
    attempts: 0,
    since_ms: Date.now(),
    ports: [
      { local: local(8088), remote: 8088 },
      { local: local(8087), remote: 8087 },
    ],
  };
  handlers?.onForward({ stack_id: stack.id, state: forwards[stack.id] });
}

// The pretend shared computer: half prepared, Set up finishes it, pairing
// shows a code that counts down.
let hostHandlers: Pick<Handlers, "onHostSnapshot" | "onHostLog"> | null = null;
let hostReady = false;
let hostArmedUntil = 0;
let hostTimer: number | null = null;
const hostSnapshot = (): HostSnapshot => {
  const remaining = Math.max(0, Math.round((hostArmedUntil - Date.now()) / 1000));
  return {
    probed: true,
    rows: [
      { name: "Windows", state: "ok", detail: "build 22631" },
      { name: "WSL 2", state: "ok", detail: "2.6.1.0" },
      MOCK_RESTART
        ? { name: "Docker Engine", state: "restart", detail: "running; restart this computer once so your login can use it" }
        : { name: "Docker Engine", state: hostReady ? "ok" : "missing", detail: hostReady ? "29.8.1" : "not installed; Set up installs it" },
      { name: "SSH server", state: hostReady ? "ok" : "missing", detail: hostReady ? "listening on 2222" : "not listening on 2222" },
      { name: "Firewall", state: hostReady ? "ok" : "missing", detail: hostReady ? "ports open" : "ports closed (Set up opens them)" },
      {
        name: "Network",
        state: hostReady ? "ok" : "missing",
        detail: hostReady ? "Home Wi-Fi (Private)" : "Home Wi-Fi is marked Public, which blocks the firewall rules",
      },
    ],
    user: "alex",
    ssh_port: 2222,
    ready_for_pairing: hostReady,
    total_memory_gb: 15,
    network: { name: "Home Wi-Fi", public: !hostReady },
    addresses: ["192.0.2.10"],
    setup_running: false,
    notice: null,
    pairing: { listening: hostReady, armed: remaining > 0, locked: false, code: remaining > 0 ? "481 923" : null, remaining_s: remaining, note: null },
    paired: hostReady ? pairedComputers.filter((computer) => !forgotten.includes(computer.address)) : [],
    host_fingerprint: hostReady ? "SHA256:TxEHBXXWzSnbkxqbV6FxMRokiF0jHb/hTzN/O0wkqU8" : null,
    connected: hostReady ? [{ address: "192.0.2.20", name: "desk", since_ms: Date.now() - 25 * 60_000 }] : [],
  };
};
const pairedComputers: PairedComputer[] = [
  {
    name: "desk",
    address: "192.0.2.20",
    key_type: "ssh-ed25519",
    paired_at_ms: Date.now() - 3 * 86400_000,
    mark: "dockernanny:4f1c2a9e7b30",
    fingerprint: "SHA256:q3Vd8yJmXbC1sR0fT6uWkN2pL9eHgA4zYxO7iUcMvE5",
  },
  // Paired by an older version: no mark and no fingerprint.
  { name: "", address: "192.0.2.21", key_type: "ssh-rsa", paired_at_ms: Date.now() - 3600_000, mark: "", fingerprint: "" },
];
const forgotten: string[] = [];
const publishHost = () => hostHandlers?.onHostSnapshot(hostSnapshot());
const hostSay = (line: string) => hostHandlers?.onHostLog({ line });

export const mockApi: Api = {
  listMachines: async () => machines,
  machineStats: async () => stats,
  appHome: async () => "/home/alex/.dockernanny",
  terminalInfo: async (machineId) => {
    const machine = machines.find((m) => m.id === machineId);
    const slug = (machine?.name ?? "stack").toLowerCase().replace(/[^a-z0-9_]+/g, "-");
    return {
      ssh_config: "/home/alex/.dockernanny/ssh_config",
      wsl_distro: MOCK_OS === "windows" ? "Ubuntu" : null,
      alias: `dn-${machineId}`,
      context_name: `dn-${slug}`,
    };
  },
  wslDistros: async () => (MOCK_OS === "windows" ? ["Ubuntu", "Ubuntu-24.04", "Debian"] : []),
  computerName: async () => "desk",
  computerInfo: async () => ({
    name: "desk",
    user: "alex",
    probe: {
      hostname: "desk",
      os: "macOS 15.1",
      cpu_model: "Apple M2",
      cpus: 8,
      load1: 2.1,
      uptime_s: 3 * 86400 + 5 * 3600,
      mem_used_mb: 11800,
      mem_total_mb: 16384,
      disk_free_bytes: 210e9,
      disk_total_bytes: 494e9,
      battery: { percent: 64, charging: false },
      docker_version: "29.7.2",
      flavor: "Docker Desktop",
      containers_running: 9,
    },
  }),
  newMachineId: async () => Math.random().toString(16).slice(2, 10),
  defaultKeyPath: async () => KEY_PATH,
  computerReadiness: async () => {
    await wait(400);
    const wsl: DoctorRow[] =
      MOCK_OS === "windows" ? [{ key: "wsl", label: "WSL", ok: true, detail: "Ubuntu, Linux 6.6.87.2-microsoft-standard-WSL2", fix: null }] : [];
    // On Windows the mock starts without rsync in WSL, so the install button shows.
    const rsyncOk = MOCK_OS !== "windows" || wslToolsInstalled;
    return [
      ...wsl,
      { key: "ssh", label: "SSH client", ok: true, detail: "OpenSSH_9.8p1", fix: null },
      {
        key: "rsync",
        label: "rsync",
        ok: rsyncOk,
        detail: rsyncOk ? "rsync  version 3.2.7  protocol version 31" : "not found",
        fix: rsyncOk ? null : "dockerNanny runs rsync inside WSL: open the distribution and run `sudo apt install rsync`.",
      },
      {
        key: "key",
        label: "SSH key",
        ok: keyMade,
        detail: keyMade ? KEY_PATH : `no key at ${KEY_PATH}`,
        fix: keyMade ? null : "Create one with the button below, or choose an existing key in Settings.",
      },
      { key: "docker", label: "Docker here", ok: true, detail: "Docker 27.3.1", fix: null },
    ];
  },
  installWslTools: async () => {
    await wait(900);
    wslToolsInstalled = true;
  },
  generateKey: async () => {
    await wait(500);
    keyMade = true;
    return `${KEY_PATH}.pub`;
  },
  keyExists: async (path) => keyMade && path === KEY_PATH,
  diagnostics: async () =>
    "dockerNanny 0.3.0 on macos aarch64\nroles: use other machines on, share this computer off\nmachines: 2 (1 online), stacks: 1\nssh: OpenSSH_9.8p1\nrsync: rsync  version 3.2.7\ndocker: 27.3.1\n\n--- end of app.log ---\nINFO copy <this-computer> -> machine-1: done\n",
  revealAppFile: async () => {},
  openLink: async (url) => void window.open(url, "_blank"),
  // `?update=1` in the address pretends a newer release exists.
  updateStatus: async () => mockUpdateStatus(),
  checkForUpdate: async () => {
    await wait(600);
    return mockUpdateStatus();
  },
  installUpdate: async () => {
    for (let step = 0; step <= 10; step += 1) {
      await wait(150);
      handlers?.onUpdateProgress(step / 10);
    }
    window.location.reload();
  },
  doctor: async (machine) => {
    const rows: DoctorRow[] = [
      { key: "ssh", label: "SSH", ok: true, detail: `${machine.user}@${machine.host}:${machine.port} answers`, fix: null },
      { key: "docker", label: "Docker", ok: true, detail: "Docker Engine 29.0.0", fix: null },
      {
        key: "compose",
        label: "Compose",
        ok: false,
        detail: "docker: 'compose' is not a docker command",
        fix: "sudo apt-get install -y docker-compose-plugin",
      },
      { key: "rsync", label: "rsync", ok: true, detail: "rsync  version 3.2.7  protocol version 31", fix: null },
      { key: "host", label: "Host", ok: true, detail: "Linux 6.8.0, 16 cpus", fix: null },
    ];
    for (const row of rows) {
      await wait(500);
      handlers?.onDoctor({ machine_id: machine.id, row });
    }
    return rows;
  },
  // Fresh arrays on every answer, like the real backend: the store compares by reference.
  addMachine: async (machine) => {
    machines.push(machine);
    stats[machine.id] = onlineStats({ hostname: machine.name.toLowerCase() });
    return [...machines];
  },
  removeMachine: async (id) => {
    const index = machines.findIndex((m) => m.id === id);
    if (index >= 0) machines.splice(index, 1);
    delete stats[id];
    // Its stacks go with it.
    for (let at = stacks.length - 1; at >= 0; at -= 1) {
      if (stacks[at].machine_id === id) stacks.splice(at, 1);
    }
    return [...machines];
  },
  pollMachine: async (id) => stats[id],
  pairMachine: async (address, code, keyPath, name) => {
    await wait(900);
    if (code === "000000") throw "wrong code";
    const machine: Machine = {
      id: Math.random().toString(16).slice(2, 10),
      name: name || "studio",
      user: "alex",
      host: address,
      port: 2222,
      key_path: keyPath,
      docker_context: false,
      pinned: true,
    };
    machines.push(machine);
    stats[machine.id] = onlineStats({
      hostname: machine.name.toLowerCase(),
      cpus: 12,
      load1: 0.3,
      mem_used_mb: 900,
      mem_total_mb: 16000,
      battery: { percent: 91, charging: true },
      os: "Windows 11 (build 22631) · Ubuntu 24.04 LTS in WSL2",
    });
    return {
      machine,
      host_fingerprint: "SHA256:TxEHBXXWzSnbkxqbV6FxMRokiF0jHb/hTzN/O0wkqU8",
      key_fingerprint: "SHA256:8mKwq2Lr5nYcB0dFvT3hJxP7sUe1aGzR6oQiN4lWkC9",
    };
  },
  setDockerContext: async (id, enabled) => {
    const machine = machines.find((m) => m.id === id);
    if (machine) machine.docker_context = enabled;
    return [...machines];
  },

  previewCompose: async () => {
    await wait(400);
    return samplePreview;
  },
  defaultExcludes: async () => [".git", "node_modules", ".DS_Store", ".tmp"],
  busyPorts: async (ports) => ports.filter((port) => port === 8087),
  listStacks: async () => stacks,
  stackStatuses: async () => statuses,
  createStack: async (stack) => {
    stacks.push(stack);
    void pretendUp(stack);
    return [...stacks];
  },
  upStack: async (id) => {
    const stack = stacks.find((s) => s.id === id);
    if (stack) void pretendUp(stack);
  },
  stopStack: async (id) => {
    publish(id, { phase: "stopping" });
    await wait(800);
    const stopped = (statuses[id]?.services ?? []).map((s) => ({ ...s, state: "exited", health: "", readiness: "stopped" as const }));
    publish(id, { phase: "stopped", services: stopped });
  },
  downStack: async (id) => {
    publish(id, { phase: "stopping" });
    await wait(800);
    publish(id, { phase: "stopped", services: [] });
  },
  restartStack: async (id) => {
    publish(id, { phase: "starting" });
    await wait(800);
    publish(id, { phase: "running" });
  },
  removeStack: async (id) => {
    const index = stacks.findIndex((s) => s.id === id);
    if (index >= 0) stacks.splice(index, 1);
    delete statuses[id];
    delete forwards[id];
    return [...stacks];
  },
  forwardStates: async () => forwards,
  // Off at once; on again after a moment, the way the real forwarder connects.
  setForwardPorts: async (id, on) => {
    const stack = stacks.find((s) => s.id === id);
    if (!stack) throw new Error("That stack is no longer known.");
    stack.forward_ports = on;
    const ports =
      statuses[id]?.services.flatMap((s) => s.ports.map((p) => ({ local: stack.port_overrides[String(p.published)] ?? p.published, remote: p.published }))) ??
      [];
    if (!on) {
      forwards[id] = { up: false, error: null, attempts: 0, since_ms: null, ports: [] };
      handlers?.onForward({ stack_id: id, state: forwards[id] });
    } else {
      forwards[id] = { up: false, error: null, attempts: 1, since_ms: null, ports };
      handlers?.onForward({ stack_id: id, state: forwards[id] });
      window.setTimeout(() => {
        forwards[id] = { up: true, error: null, attempts: 0, since_ms: Date.now(), ports };
        handlers?.onForward({ stack_id: id, state: forwards[id] });
      }, 1500);
    }
    return [...stacks];
  },
  localProjects: async () => [
    {
      name: "shop-api",
      status: "running(3)",
      config_file: "/home/alex/projects/shop-api/docker/docker-compose.yml",
      project_dir: "/home/alex/projects/shop-api/docker",
      compose_rel: "docker-compose.yml",
      volumes: [
        { key: "pgdata", name: "shop-api_pgdata", size: "412MB", external: false },
        { key: "uploads", name: "shop-api_uploads", size: "78MB", external: false },
      ],
      ports: [5432, 9000],
      warnings: ["api: mounts /home/alex/projects/shop-api/src, which is outside the project folder and will not exist on the machine"],
    },
    {
      name: "blog",
      status: "exited(2)",
      config_file: "/home/alex/projects/blog/docker-compose.yml",
      project_dir: "/home/alex/projects/blog",
      compose_rel: "docker-compose.yml",
      volumes: [{ key: "postgres", name: "blog_postgres", size: "78MB", external: false }],
      ports: [5433, 6379],
      warnings: [],
    },
  ],
  copyPlan: async (request) => {
    await wait(600);
    const fromShop = request.project?.name === "shop-api" || stacks.some((s) => s.id === request.stack_id && s.name === "shop-api");
    return {
      from: request.source.kind === "this_computer" ? "this computer" : "Studio",
      to:
        request.destination.kind === "this_computer"
          ? "this computer"
          : (machines.find((m) => request.destination.kind === "machine" && m.id === request.destination.machine_id)?.name ?? "machine"),
      destination_exists:
        request.destination.kind === "machine" &&
        stacks.some((s) => request.destination.kind === "machine" && s.machine_id === request.destination.machine_id && s.name === request.name),
      source_running: true,
      ports: fromShop ? [3000, 5432] : [5433, 6379],
      volumes: fromShop
        ? [{ key: "pgdata", name: "shop-api_pgdata", size: "412MB", destination_name: `${request.name}_pgdata`, external: false }]
        : [{ key: "postgres", name: "blog_postgres", size: "78MB", destination_name: `${request.name}_postgres`, external: false }],
      containers: fromShop
        ? [
            {
              service: "api",
              container: "shop-api-api-1",
              anonymous_volumes: [],
              changed_paths: [
                { path: "/app/storage", entries: 14, suggested: true },
                { path: "/app/.cache", entries: 3, suggested: false },
              ],
              note: null,
            },
            {
              service: "db",
              container: "shop-api-db-1",
              anonymous_volumes: [
                { name: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", destination: "/var/lib/postgresql/data", size: "84.2MB" },
              ],
              changed_paths: [],
              note: null,
            },
          ]
        : [],
      images: fromShop ? ["shop-api-worker:local"] : [],
      notes: ["compose 2.29 on the destination", "41.9 GB free, 1.2 GB needed"],
      warnings: [],
    };
  },
  copyStack: async (request) => {
    if (request.destination.kind === "machine") {
      const machineId = request.destination.machine_id;
      let record = stacks.find((s) => s.machine_id === machineId && s.name === request.name);
      if (!record) {
        record = {
          id: Math.random().toString(16).slice(2, 10),
          name: request.name,
          machine_id: machineId,
          project_dir: request.project?.project_dir ?? "/home/alex/projects/" + request.name,
          compose_rel: "docker-compose.yml",
          excludes: [".git", "node_modules"],
          forward_ports: true,
          live_sync: false,
          port_overrides: request.port_overrides,
        };
        stacks.push(record);
      }
      const card = record;
      publish(card.id, { phase: "migrating", error: null });
      pretendCopy(card.id, request.name, "this computer", machines.find((m) => m.id === machineId)?.name ?? "machine", request.destination, request.data);
      window.setTimeout(() => void pretendUp(card), 9000);
      return { stacks: [...stacks], card_id: card.id };
    }
    const cardId = request.stack_id ?? stacks[0]?.id ?? "";
    pretendCopy(cardId, request.name, "Studio", "this computer", request.destination, request.data);
    return { stacks: [...stacks], card_id: cardId };
  },
  copyProgress: async () => Object.values(copies),
  syncStack: async (id) => {
    publish(id, { synced_at_ms: Date.now(), synced_files: 2 });
  },
  startLogs: async (id) => {
    const stack = stacks.find((s) => s.id === id);
    if (!stack) return;
    const services = statuses[id]?.services.map((s) => s.service) ?? ["web"];
    let n = 0;
    logTimer = window.setInterval(() => {
      const service = services[n % services.length];
      n += 1;
      handlers?.onStackLog({
        stack_id: id,
        lines: [
          { stream: "stdout", text: `${stack.name}-${service}-1  | ${new Date().toISOString()} request ${n} handled in ${(Math.random() * 40).toFixed(1)}ms` },
        ],
      });
    }, 700);
  },
  stopLogs: async () => {
    if (logTimer) window.clearInterval(logTimer);
    logTimer = null;
  },
  listContainers: async (machineId) => {
    await wait(400);
    return (mockContainers[machineId] ?? []).map((c) => ({ ...c }));
  },
  containerAction: async (machineId, id, action) => {
    await wait(600);
    const container = mockContainers[machineId]?.find((c) => c.id === id);
    if (!container) throw new Error("That container is gone.");
    if (action === "stop") {
      container.state = "exited";
      container.status = "Exited (0) just now";
      container.ports = "";
    } else {
      container.state = "running";
      container.status = "Up 1 second";
    }
  },
  startContainerLogs: async (_machineId, id) => {
    let n = 0;
    ctrLogTimer = window.setInterval(() => {
      n += 1;
      handlers?.onContainerLog({
        id,
        lines: [
          { stream: n % 5 === 0 ? "stderr" : "stdout", text: `${new Date().toISOString()} INFO  handled event ${n} in ${(Math.random() * 30).toFixed(1)}ms` },
        ],
      });
    }, 600);
  },
  stopContainerLogs: async () => {
    if (ctrLogTimer) window.clearInterval(ctrLogTimer);
    ctrLogTimer = null;
  },

  scriptPreview: async (request) =>
    `# dockerNanny machine setup (mock)\n$distro = '${request.distro}'\n$port = ${request.port}\n$memory = '${request.memory_gb}GB'\n$pubkey = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA alex@studio'\n# ... installs Docker, sshd, rsync, writes .wslconfig, opens the firewall`,
  scriptServe: async () => {
    window.setTimeout(() => handlers?.onScriptFetched({ from: "192.0.2.20", at_ms: Date.now() }), 4000);
    const command =
      '$f = "$env:TEMP\\dockernanny-setup.ps1"; iwr http://192.0.2.10:47431/setup-0123456789abcdef.ps1 -OutFile $f -UseBasicParsing; ' +
      "if ((Get-FileHash $f -Algorithm SHA256).Hash -eq 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad') " +
      "{ iex (Get-Content -Raw -Encoding UTF8 $f) } else { Write-Host 'The script changed on the way here, so it was not run.' -ForegroundColor Red }";
    return { addresses: ["192.0.2.10"], port: 47431, commands: [command], expires_ms: Date.now() + 30 * 60_000 };
  },
  scriptStop: async () => {},

  getSettings: async () => ({ settings: settings ?? defaultSettings(), first_run: settings === null, os: MOCK_OS }),
  saveSettings: async (next) => {
    settings = next;
    return next;
  },
  resetForwards: async () => {},

  hostSnapshot: async () => hostSnapshot(),
  hostLog: async () => ["sharing started on fake Windows"],
  hostSetup: async (options) => {
    hostSay("==> Docker, sshd, rsync inside Ubuntu");
    window.setTimeout(() => {
      hostSay(`    done: Docker may use ${options.memory_gb} GB`);
      hostReady = true;
      publishHost();
    }, 1500);
  },
  hostArmPairing: async () => {
    hostArmedUntil = Date.now() + 10 * 60 * 1000;
    hostSay("pairing on for 10 minutes");
    publishHost();
    if (hostTimer) window.clearInterval(hostTimer);
    hostTimer = window.setInterval(publishHost, 1000);
  },
  hostDisarmPairing: async () => {
    hostArmedUntil = 0;
    hostSay("pairing off");
    publishHost();
  },
  hostProbe: async () => publishHost(),
  hostForget: async (address) => {
    await wait(400);
    forgotten.push(address);
    hostSay(`forgot ${address}`);
    publishHost();
  },

  copyText: async (text) => {
    await navigator.clipboard.writeText(text).catch(() => {});
  },
  openLocal: async (port) => {
    window.open(`http://localhost:${port}`, "_blank");
  },
  pickKeyFile: async () => KEY_PATH,
  pickComposeFile: async () => samplePreview.project_dir + "/docker-compose.yml",
  subscribe(next) {
    handlers = next;
    hostHandlers = next;
    const timer = window.setInterval(() => {
      const live = stats.a1b2c3d4;
      live.load1 = Math.max(0.2, live.load1 + (Math.random() - 0.5));
      next.onStats({ machine_id: "a1b2c3d4", stats: { ...live } });
    }, 3000);
    return () => {
      window.clearInterval(timer);
      handlers = null;
      hostHandlers = null;
    };
  },
};
