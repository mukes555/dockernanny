# dockerNanny

Run your Docker Compose stacks on another computer on your network and keep
using `localhost` on this one. Drop a compose file on the window, pick a
machine, and every published port shows up here with the same number. Your
editor, your browser and your tests do not notice that anything moved.

No agent, no custom protocol. dockerNanny only uses what is already there:
`ssh`, `rsync` and the `docker` CLI on the machine.

## Two roles, one app

- **Use other machines**: this computer sends stacks elsewhere. The Stacks
  and Ports pages and the machines in the sidebar belong to this role.
- **Share this computer**: this computer runs stacks for others. The sharing
  page checks what this computer has, sets up what is missing (Docker, sshd,
  rsync, the firewall; WSL2 Ubuntu on Windows), shows the pairing code, and
  keeps the Docker host awake. With this role on, closing the window hides
  it in the tray, and "Start at login" brings it back after a reboot.

Both roles can be on at once, and any operating system can play either.
The first launch asks which roles you want; Settings changes it later.

### This computer

The band at the top of the left column is where the app runs: its name,
the logged-in user, the operating system, and a chip per role. Clicking it
opens this computer's page: the same details a machine shows (load, memory,
disk free where Docker writes, processor, uptime, battery), the compose
projects the local Docker runs with a "Copy to…" on each (the way a project
leaves for a machine), and the sharing role underneath: setup, the pairing
code, the computers that paired with this one and the ones with an ssh
session open right now. A machine record that points back at this computer
(localhost, or this computer's own host name) is not listed as a machine;
Settings shows it under Maintenance so it can be removed.

## How it works

```
this computer (dockerNanny)                        machine (Linux, macOS, or WSL2 on Windows)
  drop sheet        ssh -F ~/.dockernanny/ssh_config       sshd
  stack cards  ---- rsync -e ssh ------------------------>  ~/.dockernanny/<stack>/
               ---- docker compose up/ps/logs/down ----->  Docker Engine
  localhost:3000 <- ssh -N -L 3000:127.0.0.1:3000 --------  published port 3000
```

1. **Add a machine.** Either pair with one that shows a pairing code
   (dockerNanny with "Share this computer" on), or add any Linux box with sshd, Docker Engine, the compose
   plugin and rsync by hand. The dialog checks each requirement and shows the
   command that fixes what is missing. "Prepare another machine" is the
   in-app guide with Windows, macOS and Linux tabs.
2. **Drop a compose file** or a project folder. dockerNanny reads it with
   `docker compose config`, shows the services, the published ports and any
   warnings (a bind mount outside the folder, a UDP port, an unset variable),
   and lets you pick a different local port when one is already taken here.
3. **Run.** The project folder is mirrored with rsync (minus `.git`,
   `node_modules` and whatever else you exclude), `docker compose up -d --build`
   runs on the machine, and one `ssh -N` per stack forwards every published TCP
   port back to `localhost`. A port added or dropped changes the running ssh
   in place; if the connection drops, the forward reconnects with backoff.
   The Ports page lists every forwarded port.

Everything dockerNanny keeps lives in `~/.dockernanny` on this computer: the
machine list, the stacks, the settings, one generated ssh config, the pinned
host keys, the control sockets and two logs. On the machine,
`~/.dockernanny/<stack>` holds the synced project, and Docker holds what the
stack creates. Outside those, dockerNanny writes only what you turn on: a
Docker context per machine (from the machine's menu), the start-at-login
entry, and on a computer that shares itself, what Set up lists before it
runs.

### Copy a stack between this computer and the machines

"Copy a stack…" (or "Copy to…" on a stack card) moves a stack's **config**
(the project folder) and **data** (named volumes, anonymous volumes, folders
a container wrote) from any endpoint to any other: this computer to a
machine, a machine back to this computer, or one machine to another. Pick
what travels, and the sheet shows the plan first: the volumes with their
sizes, what each container keeps for itself, images only the source has,
and whether the destination already has that stack (its folder is then
mirrored over and its data replaced).

Streams go container to container through this computer without temp files
(`tar cz | tar xz` for volumes, `docker cp -a` for container paths, `docker
save | docker load` for images); a machine to machine copy pipes two ssh
processes together. The source can be stopped for a consistent copy and
started again, left stopped (a move, which frees its ports), or kept running.
Nothing at the source is ever deleted. Afterwards the destination is checked:
services up, ports listening.

What a copy leaves at the destination: the project folder
(`~/.dockernanny/<stack>` on a machine, the folder you picked on this
computer), the volumes as `<project>_<volume>` (an external volume keeps its
own name and is added to, not replaced), the images it sent, the containers
`compose up` created, and the small helper image (`alpine:3` unless Settings
names another) that reads and writes volumes on both ends.

While it runs, a panel shows every step the copy will take and where it
stands, the bytes going through the current stream with their speed, and
the last lines of output; at the end it says how the destination came up
and opens its page. The bytes are counted on this computer, which relays
each stream a chunk at a time instead of handing the pipe to the kernel.
The card keeps a "copying from …" link that reopens the panel.

### Look at one machine

Every machine is probed with one shell script over ssh (the same script this
computer runs on itself): host name, operating system (Windows build and
the WSL distro when it is one), processor and cores, load, memory, disk
free where Docker writes, uptime, battery with its charging state, Docker
version and running containers. The card in the left column shows the OS
mark, `user@hostname`, battery, uptime and the two gauges; picking a machine
opens its page with the rest, the connection check with the fix for anything
missing, a terminal context, and the stacks that run there with a drop
target of their own.

The **bridge** is the set of localhost ports this computer hands to a
machine for one stack (ssh forwards). Every stack card shows its bridge
live: connected with the port count and since when, connecting with the
attempt count and the last error, waiting for the stack, or off, with a
Start and Stop button. The Ports page lists every bridged port with the
same state and a Restart all.

### Pairing and security

Pairing is the one step before ssh exists. The machine shows a six digit
code; this computer sends the code and its public key to the machine's
pairing port and gets back the user, the ssh port and the machine's host
key. The host key is pinned, so later connections trust only that machine.
Both screens then show the key's fingerprint, to compare. Pairing is off on
the machine until someone turns it on there, stays on for ten minutes, and
locks after ten wrong codes. A paired computer keeps its access while
sharing is off; Forget on the sharing page takes it away.
[SECURITY.md](SECURITY.md) has the full model and how to report a problem.

### Use the same machine from your terminal

The generated ssh config gives every machine an alias. Add one line at the top
of `~/.ssh/config`:

```
Include ~/.dockernanny/ssh_config
```

Then `ssh dn-<machine id>` works, and so does a Docker context, which the
machine menu can create for you:

```bash
docker context use dn-<machine name>
```

### Logs, update checks and start at login

- **Logs.** `app.log` (the app) and `host.log` (the sharing role) are in
  `~/.dockernanny`. Each is set aside as `.1` once it passes 5 MB, so there
  are never more than two of each. "Copy diagnostics" on the Help page
  copies a report for an issue with the names of this computer, the user,
  the home folder and every machine replaced, and any other address or email
  masked.
- **What goes to the internet.** Besides ssh to your machines and the
  pairing and setup ports on your network, the app makes one request of its
  own: the update check, which reads `latest.json` from this repository's
  latest GitHub release at start and every hour. Nothing about you or your
  machines is sent with it, and "Check automatically" in Settings, Updates
  turns it off. An update downloads only when you click Install, and only a build
  signed with this project's key installs. Docker still pulls images as your
  compose files ask, and Set up installs packages from the system's own
  sources.
- **Start at login** is off until you turn it on in Settings. It is a
  LaunchAgent in `~/Library/LaunchAgents` on macOS, a Run entry in the
  current user's registry on Windows and a `.desktop` file in
  `~/.config/autostart` on Linux, and turning it off removes it. The app then
  starts hidden in the tray.

## Run it

Installers for Windows (`dockerNanny_<version>_x64-setup.exe`, or the
`.msi`), macOS (`.dmg`, Apple silicon and Intel) and Linux (`.AppImage`,
`.deb`) are on the [releases page](https://github.com/mukes555/dockernanny/releases).
The builds are not signed, so Windows and macOS warn once about an unknown
developer. On the computer that shares itself, install it, choose "Share
this computer", and let Set up do the rest.

From source:

```bash
pnpm install
pnpm tauri dev
```

The UI also runs in a plain browser with a pretend backend, handy for UI work:

```bash
pnpm dev
```

Headless examples exercise the backend against any machine without the
window (this computer works as a target when its own sshd is on):

```bash
DOCKERNANNY_HOME=~/.dockernanny-test cargo run --manifest-path src-tauri/Cargo.toml --example doctor -- <user> <host> <port> ~/.ssh/id_ed25519
DOCKERNANNY_HOME=~/.dockernanny-test cargo run --manifest-path src-tauri/Cargo.toml --example stack -- <user> <host> <port> ~/.ssh/id_ed25519 examples/sample-stack
```

Tests: `pnpm test` for the window's logic, `cargo test` in `src-tauri` for
the backend, and `scripts/smoke-local.sh` for the whole core path against
this computer's own sshd.

Build the installers with `pnpm tauri build`. CI builds and tests on macOS,
Linux and Windows for every pull request. The version lives in one place,
`src-tauri/Cargo.toml`; a `v<version>` tag produces a draft release with the
bundles for all three.

## Layout

```
src/                 React window
  app/ computer/ machines/ stacks/ host/ guide/ settings/
                       one folder per part of the window
  ui/                  shared building blocks: Page, Drawer, StatusDot, useAction, ...
  lib/                 the backend's types, the one door to it (ipc), formats
  state/store.ts       the one store
src-tauri/src/       Rust backend
  commands.rs          what the window can ask for, one file per part in commands/
  machine.rs           machines, stats poll, Docker context; doctor.rs checks them
  probe.rs             the one shell script that describes a computer, and its parsers
  stack/               what a stack is (mod), its operations (lifecycle), the ps poll
  forward.rs           the ssh -N port forwards
  sync.rs              rsync and the live re-sync watcher
  copy/                copying a stack: mod (the order), look (both ends, for plan and
                       run alike), carry, report, endpoint, folder, transfer, discover,
                       pull, check, steps, progress
  pairing.rs           the pairing exchange and its validators, shared by both roles
  host/                the sharing role: engine, pairing server, paired.json, one file per OS
  tray.rs              the tray icon and the close-to-hide behaviour
  guide.rs             the Windows setup script and its one-file web server
  ssh.rs               the generated ssh config, control masters, jobs
  store.rs             machines.json, stacks.json, settings.json, the logs
  updates.rs           the update check
scripts/             smoke-local (the core path end to end), check-windows,
                     release-local and hide-paths (release builds), privacy-audit
examples/            sample stacks to try things with
```

## Not yet

Compose profiles, several `-f` files, port ranges, `network_mode: host`, build
contexts outside the project folder, copying files back from the machine,
password logins, and finding machines on the network by themselves. The
sharing role runs after login only; it is not a service.
