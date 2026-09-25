#!/bin/sh
# Builds a portable Windows build from macOS or Linux, for trying a branch
# on a Windows computer when CI is not available: dockernanny.exe plus the
# WebView2Loader.dll it loads, zipped. No installer; unzip and run the exe.
#
# Needs once: rustup target add x86_64-pc-windows-gnu, and mingw-w64
#             (brew install mingw-w64 / apt install mingw-w64).
#
# Like release-local.sh, the build keeps this computer's paths out of the
# binary and fails if one slipped in. Output: .tmp/windows/<name>.zip
set -e
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
version="$(node -p "require('./package.json').version")"
name="dockerNanny-$version-windows-x64-portable"
target_dir="$repo_dir/src-tauri/target/windows-check"
toolchain_bin="$(dirname "$(rustup which rustc --toolchain stable)")"

export PATH="$toolchain_bin:$PATH"
export CARGO_TARGET_DIR="$target_dir"
export RUSTFLAGS="--remap-path-prefix=$HOME=/home --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$repo_dir=/dockernanny"
pnpm tauri build --target x86_64-pc-windows-gnu --no-bundle

release="$target_dir/x86_64-pc-windows-gnu/release"
if strings "$release/dockernanny.exe" | grep -q "$HOME"; then
  echo "dockernanny.exe contains $HOME; not packaging it" >&2
  exit 1
fi

out="$repo_dir/.tmp/windows"
rm -rf "${out:?}/$name" "$out/$name.zip"
mkdir -p "$out/$name"
cp "$release/dockernanny.exe" "$release/WebView2Loader.dll" "$out/$name/"
(cd "$out" && zip -qr "$name.zip" "$name" && shasum -a 256 "$name.zip")
echo "$out/$name.zip"
