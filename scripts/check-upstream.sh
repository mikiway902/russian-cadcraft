#!/usr/bin/env bash
# Build a candidate in a disposable worktree; never advance a local or remote branch.
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo 'usage: bash scripts/check-upstream.sh [repository]' >&2
  exit 2
fi
root=$(git -C "${1:-.}" rev-parse --show-toplevel)
url=${CADCRAFTGOST_UPSTREAM_URL:-https://github.com/storytold/cadcraft.git}
branch=${CADCRAFTGOST_UPSTREAM_BRANCH:-main}
git check-ref-format "refs/heads/$branch" >/dev/null
report="$root/target/upstream-check"
mkdir -p "$report"
head=$(git -C "$root" rev-parse HEAD)
if [[ $(git -C "$root" rev-parse --is-shallow-repository) == true ]]; then
  git -C "$root" fetch --unshallow --no-tags origin
fi
git -C "$root" fetch --no-tags -- "$url" "refs/heads/$branch"
upstream=$(git -C "$root" rev-parse FETCH_HEAD)
printf 'CADCraftGOST: %s\nUpstream: %s\n' "$head" "$upstream" | tee "$report/revisions.txt"
if git -C "$root" merge-base --is-ancestor "$upstream" "$head"; then
  echo 'No new upstream commits; no branch was modified.'
  exit 0
fi

# merge-tree reports conflicts without touching either the caller's index or worktree.
if ! git -C "$root" merge-tree --write-tree --messages "$head" "$upstream" >"$report/merge.txt" 2>&1; then
  cat "$report/merge.txt"
  echo 'Upstream conflicts or merge errors require manual resolution; nothing was adopted.' >&2
  exit 1
fi

# A stable source path keeps compile-time CARGO_MANIFEST_DIR valid on later runs.
# A separate target prevents candidate binaries/build scripts from poisoning the caller's cache.
lock="$report/run.lock"
if ! mkdir "$lock"; then
  echo "Another upstream check owns $lock; do not remove its worktree." >&2
  exit 1
fi
candidate="$root/target/upstream-candidate"
owned=false
cleanup() {
  local status=$?
  trap - EXIT
  if [[ "$owned" = true && -e "$candidate/.git" ]]; then
    # Only our disposable candidate may contain generated files or an uncommitted test merge.
    if ! git -C "$root" worktree remove --force "$candidate"; then
      echo "Could not remove disposable worktree: $candidate" >&2
      status=1
    fi
  fi
  if ! rmdir "$lock"; then
    echo "Could not remove owned run lock: $lock" >&2
    status=1
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [[ -e "$candidate" ]]; then
  echo "Candidate path already exists: $candidate; inspect it manually, nothing was removed." >&2
  exit 1
fi
git -C "$root" worktree add --detach "$candidate" "$head"
owned=true
git -C "$candidate" -c core.hooksPath=/dev/null \
  -c user.name='CADCraftGOST upstream check' \
  -c user.email='upstream-check@users.noreply.github.com' \
  merge --no-commit --no-ff "$upstream"
(
  cd "$candidate"
  # Never inherit the caller's CARGO_TARGET_DIR: root and candidate compile-time paths differ.
  export CARGO_TARGET_DIR="$root/target/upstream-build"
  cargo xtask ci
) 2>&1 | tee "$report/gate.log"
echo 'Candidate passed the complete gate. No branch was advanced or pushed; adoption requires review.'
