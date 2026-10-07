#!/bin/sh
# The core path, end to end, against this computer's own sshd: the machine
# check, a stack run (rsync, compose up, ps, down) and a copy there and back
# that must keep a database row written before it. Run it before a release
# and after any change to ssh, rsync, compose or copy code.
#
# Needs: Docker running here; Remote Login (macOS) or sshd (Linux) on port
# 22; your public key in ~/.ssh/authorized_keys. KEY=<private key> picks
# another key. Uses port 5499 and 8087/8088 while it runs, and two free
# ports the system picks for the bridge check.
#
# Everything it creates is named smoke-* and removed at the end: the app
# folder in ~/.dockernanny-smoke, the stacks' containers and volumes, and the
# folders the "machine" side keeps in ~/.dockernanny/smoke-*.
set -e
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
key="${KEY:-$HOME/.ssh/id_ed25519}"
user="$(whoami)"
tmp="$repo_dir/.tmp/smoke"
# Short on purpose: ssh control sockets must fit in 104 bytes.
export DOCKERNANNY_HOME="$HOME/.dockernanny-smoke"

say() { printf '\n==> %s\n' "$1"; }
fail() { printf '\nSMOKE FAILED: %s\n' "$1" >&2; exit 1; }
example() { (cd "$repo_dir/src-tauri" && cargo run -q --example "$@"); }
row_in() { docker compose -p "$1" -f "$2/docker-compose.yml" exec -T db psql -U postgres -tAc "select note from moved" 2>/dev/null; }
has_the_row() { [ "$(row_in "$1" "$2")" = "smoke row" ]; }
postgres_ready() { docker compose -p smoke-anon -f "$tmp/anon/docker-compose.yml" exec -T db pg_isready -U postgres >/dev/null 2>&1; }

# Asks once a second until the answer is yes; after 30 tries the smoke test fails with $1.
wait_until() {
  what="$1"
  shift
  tries=0
  until "$@"; do
    tries=$((tries + 1))
    [ "$tries" -gt 30 ] && fail "$what"
    sleep 1
  done
}

cleanup() {
  machine_side="$HOME/.dockernanny"
  for project in smoke-anon:"$tmp/anon" smoke-anon-m:"$machine_side/smoke-anon-m" smoke-anon-back:"$tmp/back" smoke-stack:"$machine_side/smoke-stack"; do
    name="${project%%:*}"
    dir="${project#*:}"
    [ -f "$dir/docker-compose.yml" ] && docker compose -p "$name" -f "$dir/docker-compose.yml" down -v >/dev/null 2>&1 || true
  done
  rm -rf "$tmp" "$DOCKERNANNY_HOME" "$machine_side/smoke-anon-m" "$machine_side/smoke-stack"
}
trap cleanup EXIT
cleanup
mkdir -p "$tmp/anon"

say "machine check (doctor) against $user@127.0.0.1:22"
out="$(example doctor "$user" 127.0.0.1 22 "$key")"
echo "$out" | grep '^ERR' && fail "a doctor check failed"
echo "$out" | grep -q '^ok  SSH' || fail "ssh to this computer did not answer"

say "giving up on a remote command ends it on the machine"
out="$(example cancel "$user" 127.0.0.1 22 "$key")" || fail "a cancelled or timed-out command was left running"
echo "$out" | grep -c 'left running = 0' | grep -q '^2$' || fail "expected two 'left running = 0' lines"

say "port bridge: up, a port added and one dropped in place, then ended"
out="$(example bridge "$user" 127.0.0.1 22 "$key" 2>&1)" || { echo "$out"; fail "the port bridge broke a promise"; }

say "stack: sync, up, ps, down"
out="$(example stack "$user" 127.0.0.1 22 "$key" "$repo_dir/examples/sample-stack")"
echo "$out" | sed -n '/^== up/,/^== ps/p' | grep -q 'exit Some(0)' || fail "compose up failed"
echo "$out" | grep -q 'web running' || fail "the web service did not run"

say "copy: a database row there and back"
cp "$repo_dir/examples/anon-volume-stack/docker-compose.yml" "$tmp/anon/"
docker compose -p smoke-anon -f "$tmp/anon/docker-compose.yml" up -d >/dev/null 2>&1
wait_until "postgres did not start" postgres_ready
docker compose -p smoke-anon -f "$tmp/anon/docker-compose.yml" exec -T db psql -U postgres -c "create table moved(note text); insert into moved values ('smoke row');" >/dev/null
example copy "$user" 127.0.0.1 22 "$key" local:smoke-anon machine:smoke-anon-m both leave >/dev/null
wait_until "the row did not arrive on the machine" has_the_row smoke-anon-m "$HOME/.dockernanny/smoke-anon-m"
example copy "$user" 127.0.0.1 22 "$key" machine:smoke-anon-m "local:smoke-anon-back:$tmp/back" both leave >/dev/null
wait_until "the row did not come back" has_the_row smoke-anon-back "$tmp/back"

say "all good"
