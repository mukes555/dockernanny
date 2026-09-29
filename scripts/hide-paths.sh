#!/bin/sh
# Rust keeps the source path of every crate in a release binary (panic
# locations survive stripping), so a build would carry the build computer's
# home and checkout inside it. Both release builds, scripts/release-local.sh
# and the release workflow, hide them here, the same way:
#
#   scripts/hide-paths.sh flags <home> <checkout>            the RUSTFLAGS that replace them
#   scripts/hide-paths.sh check <home> <checkout> <file>...  fails when a file still holds one
set -eu

if [ $# -lt 3 ]; then
  echo "usage: hide-paths.sh flags|check <home> <checkout> [file...]" >&2
  exit 2
fi
command=$1
home=$2
checkout=$3
shift 3

case "$command" in
  flags)
    # rustc applies the last matching remap, so the general one goes first.
    echo "--remap-path-prefix=$home=/home --remap-path-prefix=$home/.cargo=/cargo --remap-path-prefix=$checkout=/dockernanny"
    ;;
  check)
    [ $# -gt 0 ] || { echo "no binaries to check" >&2; exit 1; }
    leaks=0
    for file in "$@"; do
      for path in "$home" "$checkout"; do
        count=$(grep -a -c -F -- "$path" "$file" || true)
        if [ "$count" -gt 0 ]; then
          echo "$file still holds $path ($count places)"
          leaks=1
        fi
      done
    done
    if [ "$leaks" -ne 0 ]; then
      echo "paths from the build computer are still inside the build" >&2
      exit 1
    fi
    echo "no paths from the build computer in: $*"
    ;;
  *)
    echo "usage: hide-paths.sh flags|check <home> <checkout> [file...]" >&2
    exit 2
    ;;
esac
