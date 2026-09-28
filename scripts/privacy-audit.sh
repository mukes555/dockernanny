#!/usr/bin/env bash
# Looks for personal details before anything is published: in the tracked
# files, optionally in every commit ever made, in the repository's text on
# GitHub, and in built binaries.
#
# The terms to look for are yours, so they live in a file that is never
# committed: `.privacy-denylist` at the repo root (gitignored), one term per
# line, `#` for comments. A term matches case-insensitively anywhere. A line
# `word:Name` matches only that exact word, for short names that would
# otherwise match inside other words (`word:Kit` finds "Kit", not "kitchen").
#
#   scripts/privacy-audit.sh                  tracked files
#   scripts/privacy-audit.sh --history        also every commit's content and message
#   scripts/privacy-audit.sh --github         also pull requests, issues, release notes
#                                             and each release's update feed (needs gh)
#   scripts/privacy-audit.sh --binary <file>  also a built binary (plus your home path)
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
denylist="$repo_dir/.privacy-denylist"
cd "$repo_dir"

if [ ! -s "$denylist" ]; then
  echo "no $denylist: add the names, hosts, paths and emails that must never be published" >&2
  exit 2
fi
# Two pattern files: terms matched anywhere, words matched whole and exact.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
grep -v -e '^\s*#' -e '^\s*$' -e '^word:' "$denylist" > "$work/terms" || true
sed -n 's/^word://p' "$denylist" > "$work/words"

# The lines of a file that hold a term or a word.
matching_lines() {
  if [ -s "$work/terms" ]; then grep -h -i -F -f "$work/terms" "$1" || true; fi
  if [ -s "$work/words" ]; then grep -h -w -F -f "$work/words" "$1" || true; fi
}

count_in() {
  matching_lines "$1" | wc -l | tr -d ' '
}

check_history=0
check_github=0
binaries=()
while [ $# -gt 0 ]; do
  case "$1" in
    --history) check_history=1 ;;
    --github) check_github=1 ;;
    --binary) shift; binaries+=("$1") ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

found=0
not_files=(':!pnpm-lock.yaml' ':!src-tauri/Cargo.lock')

echo "==> tracked files"
tracked="$(
  if [ -s "$work/terms" ]; then git grep -n -I -i -F -f "$work/terms" -- . "${not_files[@]}" || true; fi
  if [ -s "$work/words" ]; then git grep -n -I -w -F -f "$work/words" -- . "${not_files[@]}" || true; fi
)"
if [ -n "$tracked" ]; then
  echo "$tracked"
  found=1
else
  echo "   clean"
fi

if [ "$check_history" -eq 1 ]; then
  echo "==> every commit (contents and messages)"
  git log --all -p --no-color > "$work/history"
  git log --all --format='%an %ae %cn %ce' > "$work/authors"
  history_hits=$(count_in "$work/history")
  author_hits=$(count_in "$work/authors")
  if [ "$history_hits" -gt 0 ] || [ "$author_hits" -gt 0 ]; then
    echo "   $history_hits matching lines in diffs and messages, $author_hits in author or committer fields"
    found=1
  else
    echo "   clean"
  fi
fi

if [ "$check_github" -eq 1 ]; then
  echo "==> GitHub: pull requests, issues, release notes and the update feed"
  repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
  text="$work/github"
  gh pr list -R "$repo" --state all --limit 1000 --json number,title,body,comments \
    -q '.[] | "PR #\(.number): \(.title)\n\(.body)\n\([.comments[].body] | join("\n"))"' > "$text"
  gh issue list -R "$repo" --state all --limit 1000 --json number,title,body,comments \
    -q '.[] | "issue #\(.number): \(.title)\n\(.body)\n\([.comments[].body] | join("\n"))"' >> "$text"
  for tag in $(gh release list -R "$repo" --limit 1000 --json tagName -q '.[].tagName'); do
    { echo "release $tag"; gh release view "$tag" -R "$repo" --json body -q .body; } >> "$text"
    # The notes the app shows for an update come from this file.
    gh release download "$tag" -R "$repo" -p latest.json -O - >> "$text" 2>/dev/null || true
  done
  # Every line carries where it came from, so a match says where to fix it.
  awk '/^(PR #|issue #|release )/ { where = $0 } { print where " :: " $0 }' "$text" > "$work/github-labelled"
  github_hits="$(matching_lines "$work/github-labelled")"
  if [ -n "$github_hits" ]; then
    echo "$github_hits" | cut -c1-200 | head -40
    found=1
  else
    echo "   clean"
  fi
fi

for binary in "${binaries[@]+"${binaries[@]}"}"; do
  echo "==> $binary"
  strings -a "$binary" > "$work/strings"
  term_hits=$(count_in "$work/strings")
  home_hits=$(grep -c -F "$HOME" "$work/strings" || true)
  if [ "$term_hits" -gt 0 ] || [ "$home_hits" -gt 0 ]; then
    echo "   $term_hits deny-list matches, $home_hits copies of $HOME"
    # The strings themselves, so a real name tells itself apart from a short
    # word that compressed bytes happen to spell out (like "x<Ab" for a word:Ab).
    { matching_lines "$work/strings"; grep -F "$HOME" "$work/strings" || true; } | cut -c1-120 | head -20 | sed 's/^/     /'
    found=1
  else
    echo "   clean"
  fi
done

exit "$found"
