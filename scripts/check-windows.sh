#!/bin/sh
# Type-checks and lints the Windows build from macOS or Linux, so code behind
# #[cfg(windows)] is compiled before it reaches a Windows computer. Nothing is
# linked, so no Windows SDK is needed.
#
# Needs once: rustup target add x86_64-pc-windows-gnu
#             and mingw-w64 (brew install mingw-w64 / apt install mingw-w64),
#             whose gcc a few dependencies' build scripts call.
#
# rustup's toolchain goes first in PATH: a Homebrew or distro rustc has no
# Windows target, and mixing two compilers in one target folder breaks the
# build. The separate target folder keeps it apart from the everyday build.
set -e
cd "$(dirname "$0")/../src-tauri"
toolchain_bin="$(dirname "$(rustup which rustc --toolchain stable)")"
PATH="$toolchain_bin:$PATH" CARGO_TARGET_DIR=target/windows-check \
  cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings
