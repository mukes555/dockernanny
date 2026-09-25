#!/usr/bin/env bash
# Looks for personal details before anything is published: in the tracked
# files, optionally in every commit ever made, and in built binaries.
#
# The terms to look for are yours, so they live in a file that is never
# committed: `.privacy-denylist` at the repo root (gitignored), one term per
# line, `#` for comments. Matching is case-insensitive and literal.
#
#   scripts/privacy-audit.sh                  tracked files
#   scripts/privacy-audit.sh --history        also every commit's content and message
#   scripts/privacy-audit.sh --binary <file>  also a built binary (plus your home path)
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
denylist="$repo_dir/.privacy-denylist"
cd "$repo_dir"

if [ ! -s "$denylist" ]; then
  echo "no $denylist: add the names, hosts, paths and emails that must never be published" >&2
  exit 2
fi
# Comments and blank lines out, so the patterns file holds only terms.
patterns="$(mktemp)"
trap 'rm -f "$patterns"' EXIT
grep -v -e '^\s*#' -e '^\s*$' "$denylist" > "$patterns"

check_history=0
binaries=()
while [ $# -gt 0 ]; do
  case "$1" in
    --history) check_history=1 ;;
    --binary) shift; binaries+=("$1") ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

found=0

echo "==> tracked files"
if git grep -n -I -i -F -f "$patterns" -- . ':!pnpm-lock.yaml' ':!src-tauri/Cargo.lock'; then
  found=1
else
  echo "   clean"
fi

if [ "$check_history" -eq 1 ]; then
  echo "==> every commit (contents and messages)"
  history_hits=$(git log --all -p --no-color | grep -c -i -F -f "$patterns" || true)
  author_hits=$(git log --all --format='%an %ae %cn %ce' | grep -c -i -F -f "$patterns" || true)
  if [ "$history_hits" -gt 0 ] || [ "$author_hits" -gt 0 ]; then
    echo "   $history_hits matching lines in diffs and messages, $author_hits in author or committer fields"
    found=1
  else
    echo "   clean"
  fi
fi

for binary in "${binaries[@]+"${binaries[@]}"}"; do
  echo "==> $binary"
  term_hits=$(strings -a "$binary" | grep -c -i -F -f "$patterns" || true)
  home_hits=$(strings -a "$binary" | grep -c -F "$HOME" || true)
  if [ "$term_hits" -gt 0 ] || [ "$home_hits" -gt 0 ]; then
    echo "   $term_hits deny-list matches, $home_hits copies of $HOME"
    found=1
  else
    echo "   clean"
  fi
done

exit "$found"
