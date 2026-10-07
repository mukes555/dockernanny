#!/usr/bin/env bash
# Builds the installers for this computer's platform without CI, with no
# trace of this computer's paths inside them, and writes SHA256SUMS.
# scripts/hide-paths.sh replaces the paths while it builds and fails the
# build if any are left.
#
#   scripts/release-local.sh
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
bundle_dir="$repo_dir/src-tauri/target/release/bundle"
cd "$repo_dir"

RUSTFLAGS="$(sh scripts/hide-paths.sh flags "$HOME" "$repo_dir")"
export RUSTFLAGS

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
binaries=()
while IFS= read -r binary; do
  binaries+=("$binary")
done < <(find "$repo_dir/src-tauri/target/release" -maxdepth 1 -type f -perm -u+x -name 'dockernanny*')
sh scripts/hide-paths.sh check "$HOME" "$repo_dir" "${binaries[@]}"

echo "==> writing SHA256SUMS"
# Only the finished installers: the bundler leaves temporary rw.*.dmg files behind.
cd "$bundle_dir"
find dmg nsis msi appimage deb rpm -maxdepth 1 -type f ! -name 'rw.*' \
  \( -name '*.dmg' -o -name '*.exe' -o -name '*.msi' -o -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) -print0 2>/dev/null \
  | xargs -0 shasum -a 256 > SHA256SUMS
cat SHA256SUMS
