# Contributing

Thanks for looking at dockerNanny. Small, focused pull requests are the
easiest to review.

## Setup

- Node 22 with pnpm, Rust stable, and the Tauri 2 prerequisites for your OS
  (https://v2.tauri.app/start/prerequisites/).
- `pnpm install`, then `pnpm tauri dev` for the app, or `pnpm dev` for the UI
  alone in a browser with a pretend backend.
- `cargo test` and `cargo clippy --all-targets -- -D warnings` in `src-tauri/`,
  `pnpm typecheck` and `pnpm build` for the UI. CI runs the same on Linux and
  Windows for every pull request.

## How the code is laid out

- `src/`: React UI. One file per screen or sheet, shared pieces in `src/ui`.
- `src-tauri/src/`: the Rust side. `ssh.rs` is the only place that spawns ssh;
  `compose.rs` reads compose files; `stack/` runs a stack's life; `forward.rs`
  keeps localhost ports pointing at a machine; `copy/` moves config and data
  between endpoints; `host/` is the sharing role; `guide.rs` and `pairing.rs`
  prepare and pair machines.
- `src-tauri/examples/`: headless checks against a real machine, useful when
  a change touches ssh, rsync or Docker.

## Style

- Write for the reader: early returns, named intermediate conditions, comments
  that say why. Keep files under about 300 lines.
- No personal data in the repository: use RFC 5737 addresses (`192.0.2.x`),
  made up names and `/home/alex/...` paths in mocks, tests and docs. Before
  publishing, `scripts/privacy-audit.sh --github` checks the code and the
  repository's text on GitHub against your own `.privacy-denylist`, a
  gitignored list of the names, hosts and paths that are yours.
- User facing words: "this computer" for where the app runs, "machine" for the
  other side. Never assume an operating system unless the text is about one.
