# Security

## What dockerNanny assumes

dockerNanny is built for machines you own on a network you trust, such as a
home or office LAN. It moves your project files and your Docker data between
those machines, so read this before using it anywhere else.

- **Transport.** Everything between machines goes over OpenSSH: file sync
  (rsync), Docker commands, port forwards and volume streams. dockerNanny never
  opens container ports on the network; they are reachable only on
  `localhost` of the computer that forwards them.
- **Pairing** is the one step that happens before SSH exists. The machine
  shows a six digit code; the computer that wants to use it connects to TCP
  port 47433 on the machine (Settings can change it), sends the code and its
  SSH public key, and gets back the user name, the SSH port and the machine's
  SSH host key. The connection is plain TCP: anyone on the LAN can see the
  public key, which is public by design. A wrong code is answered with a one
  second delay; after ten wrong codes the machine stops listening until you
  turn pairing on again. Pairing is off unless you turn it on, and turns
  itself off after ten minutes.
  Afterwards both screens show the fingerprint of the key that was added;
  when they match, nobody stood in between.
- **Paired keys.** The key goes into `~/.ssh/authorized_keys` on the machine
  with the comment `dockernanny:<id>`. Forget on the sharing page removes
  that one line and nothing else; a key paired by a version from before the
  marks has to be removed by hand, and the page says so. Turning sharing off
  does not take access away; Forget does.
- **Host keys.** A paired machine's SSH host key is pinned at pairing time in
  `~/.dockernanny/known_hosts`. Machines added by hand use `accept-new`: the
  first connection trusts whatever answers at that address.
- **The Windows setup script** reaches a Windows machine before SSH exists,
  over plain HTTP from this computer (port 47431 by default), and runs as
  Administrator. So the one line you type there checks it: the address has a
  random part nobody on the network can guess, the line compares the file's
  SHA-256 with the one this computer shows before running it, and the server
  stops by itself after 30 minutes.
- **The window** can ask the app only for its own commands, and of Tauri's
  features only for events, dragging the window, one file dialog, opening
  web links and copying text.
- **Where things are written.** On the computer: `~/.dockernanny`, plus a
  Docker context if you ask for one and the start-at-login entry while that
  setting is on. On a machine: `~/.dockernanny/<stack>` for synced projects,
  Docker volumes, and, for the sharing role, the changes listed in the setup
  screen before they are made (Docker, sshd, rsync, firewall rules for the
  SSH and pairing ports, power settings) and the paired keys above.
- **What goes to the internet.** The update check reads `latest.json` from
  this repository's latest GitHub release at start and every hour, unless
  it is turned off in Settings. It sends nothing about you or your machines.
  An update installs only when you click Install, and only if it is signed
  with this project's key.
- **Logs.** `app.log` and `host.log` in `~/.dockernanny` stay on this
  computer. They can name your machines and addresses; the diagnostics
  report the Help page copies replaces those before you share it.
- **Secrets.** `docker compose config` output can contain `env_file` values;
  dockerNanny reads it in memory to show the preview and never stores it. The
  `.env` file of a project is synced to the machine with the project, and the
  app says so before running.

## Reporting a vulnerability

Open a private security advisory on the GitHub repository, or email the
maintainers at the address in the repository profile. Please include the
version, the operating systems on both sides, and steps to reproduce. You will
get an answer within a week.
