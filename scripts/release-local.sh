#!/usr/bin/env bash
# Builds the installers for this computer's platform without CI, with no
# trace of this computer's paths inside them, and writes SHA256SUMS.
#
# Rust keeps the source path of every crate in a release binary (panic
# locations survive stripping), so without remapping a build made here would
# carry /Users/<you>/.cargo/... inside it. The remaps below replace those
# prefixes; the check at the end fails the build if any are left.
#
#   scripts/release-local.sh
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
bundle_dir="$repo_dir/src-tauri/target/release/bundle"
cd "$repo_dir"

# rustc applies the last matching remap, so the general one goes first.
export RUSTFLAGS="--remap-path-prefix=$HOME=/home --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$repo_dir=/dockernanny"

# The updater artifacts must be signed with the key installed apps trust.
# It lives outside the repository; CI has the same key as a secret.
key="${TAURI_KEY_FILE:-$HOME/.tauri/dockernanny.key}"
if [ ! -f "$key" ]; then
  echo "no updater signing key at $key (set TAURI_KEY_FILE); the release workflow on GitHub signs with the repository secret instead" >&2
  exit 1
fi
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$key.password" 2>/dev/null || true)"

echo "==> building with path remapping"
pnpm tauri build

echo "==> checking the binaries for this computer's paths"
leaks=0
while IFS= read -r binary; do
  count=$(strings -a "$binary" | grep -c -F "$HOME" || true)
  if [ "$count" -gt 0 ]; then
    echo "   $binary still contains $count copies of $HOME"
    leaks=1
  fi
done < <(find "$repo_dir/src-tauri/target/release" -maxdepth 1 -type f -perm -u+x -name 'dockernanny*')
if [ "$leaks" -ne 0 ]; then
  echo "refusing to publish: paths from this computer are still inside the build" >&2
  exit 1
fi
echo "   none found"

echo "==> writing SHA256SUMS"
# Only the finished installers: the bundler leaves temporary rw.*.dmg files behind.
cd "$bundle_dir"
find dmg nsis msi appimage deb rpm -maxdepth 1 -type f ! -name 'rw.*' \
  \( -name '*.dmg' -o -name '*.exe' -o -name '*.msi' -o -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) -print0 2>/dev/null \
  | xargs -0 shasum -a 256 > SHA256SUMS
cat SHA256SUMS
