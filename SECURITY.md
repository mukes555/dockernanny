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
  port 47433 on the machine, sends the code and its SSH public key, and gets
  back the user name, the SSH port and the machine's SSH host key. The
  connection is plain TCP: anyone on the LAN can see the public key, which is
  public by design. A wrong code is answered with a one second delay; after
  ten wrong codes the machine stops listening until you turn pairing on again.
  Pairing is off unless you turn it on, and turns itself off after ten minutes.
- **Host keys.** A paired machine's SSH host key is pinned at pairing time in
  `~/.dockernanny/known_hosts`. Machines added by hand use `accept-new`: the
  first connection trusts whatever answers at that address.
- **Where things are written.** On the computer: `~/.dockernanny` only, plus
  an optional Docker context if you ask for one. On a machine:
  `~/.dockernanny/<stack>` for synced projects, Docker volumes, and, for the
  host role, the changes listed in the setup screen before they are made
  (Docker, sshd, rsync, firewall rules for the SSH and pairing ports, power
  settings).
- **Secrets.** `docker compose config` output can contain `env_file` values;
  dockerNanny reads it in memory to show the preview and never stores it. The
  `.env` file of a project is synced to the machine with the project, and the
  app says so before running.

## Reporting a vulnerability

Open a private security advisory on the GitHub repository, or email the
maintainers at the address in the repository profile. Please include the
version, the operating systems on both sides, and steps to reproduce. You will
get an answer within a week.
