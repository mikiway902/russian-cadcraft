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

scratch=$(mktemp -d "${TMPDIR:-/tmp}/cadcraftgost-upstream.XXXXXXXX")
candidate="$scratch/candidate"
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -e "$candidate/.git" ]]; then
    # Only our disposable candidate may contain generated files or an uncommitted test merge.
    if ! git -C "$root" worktree remove --force "$candidate"; then
      echo "Could not remove disposable worktree: $candidate" >&2
      status=1
    fi
  fi
  if ! rmdir "$scratch"; then
    echo "Could not remove disposable directory: $scratch" >&2
    status=1
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

git -C "$root" worktree add --detach "$candidate" "$head"
git -C "$candidate" -c core.hooksPath=/dev/null \
  -c user.name='CADCraftGOST upstream check' \
  -c user.email='upstream-check@users.noreply.github.com' \
  merge --no-commit --no-ff "$upstream"
(
  cd "$candidate"
  # Share compiled dependencies, not the caller's source/index or persisted drawing data.
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"
  cargo xtask ci
) 2>&1 | tee "$report/gate.log"
echo 'Candidate passed the complete gate. No branch was advanced or pushed; adoption requires review.'
