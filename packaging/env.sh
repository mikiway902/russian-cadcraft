# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
#
# Exports:
#   ROOT                    workspace root
#   VERSION                 [workspace.package] version from Cargo.toml (override: CADCRAFT_VERSION)
#   DIST                    output directory for release artifacts (default: $ROOT/dist/release)
#   CADCRAFT_BUILD_SHA    git commit baked into the binaries (see crates/engine/src/build_info.rs)
#   CADCRAFT_BUILD_DATE   UTC build date, YYYY-MM-DD
#   CARGO_TARGET_DIR        cargo's target dir (default: $ROOT/target)
#   MODEL_WORKER_TARGET_DIR  isolated exact-model worker build directory ($CARGO_TARGET_DIR/model-worker)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in exactly one place: `[workspace.package] version` in the root Cargo.toml.
# (`cargo xtask version` prints the same thing; awk avoids compiling xtask here.)
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
  return
}

VERSION="${CADCRAFT_VERSION:-$(workspace_version)}"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST

if [[ -z "${CADCRAFT_BUILD_SHA:-}" ]]; then
  CADCRAFT_BUILD_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)"
fi
export CADCRAFT_BUILD_SHA
export CADCRAFT_BUILD_DATE="${CADCRAFT_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
# Cargo resolves relative target directories from the workspace being built. Use one absolute
# base for both independent workspaces, including callers that package from another directory.
case "$CARGO_TARGET_DIR" in /*) ;; *) CARGO_TARGET_DIR="$ROOT/$CARGO_TARGET_DIR" ;; esac
export CARGO_TARGET_DIR
export MODEL_WORKER_TARGET_DIR="$CARGO_TARGET_DIR/model-worker"

# The worker remains outside the host Cargo workspace and uses its own committed lockfile.
# An explicit target follows each platform's existing host/cross/universal build contract.
build_model_worker() {
  local target="${1:-}"
  local args=()
  [[ -z "$target" ]] || args+=(--target "$target")
  (cd "$ROOT" && cargo build --release --locked \
    --manifest-path "$ROOT/tools/solidcraft-bridge/Cargo.toml" --bin cadcraft-model-worker \
    --target-dir "$MODEL_WORKER_TARGET_DIR" ${args[@]+"${args[@]}"})
}

model_worker_binary() {
  local target="${1:-}"
  local binary="$MODEL_WORKER_TARGET_DIR/${target:+$target/}release/cadcraft-model-worker"
  if [[ ! -f "$binary" || ! -x "$binary" ]]; then
    echo "error: required exact-model worker missing or not executable: $binary; build without --skip-build" >&2
    return 1
  fi
  printf '%s\n' "$binary"
}

# These are full copyright/licence texts, not the dependency SPDX inventory. Keep them with
# every native worker, including --skip-build packages and the standalone macOS CLI archive.
copy_model_worker_licences() {
  local dest="$1"
  mkdir -p "$dest/model-worker-licences"
  cp "$ROOT/packaging/model-worker-licences/"*.txt "$dest/model-worker-licences/"
}

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
  return
}

# Copy licence and readme files that exist into a package directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE LICENSE-MIT LICENSE-APACHE COPYRIGHT NOTICE ATTRIBUTION.md; do
    if [[ -f "$ROOT/$f" ]]; then cp "$ROOT/$f" "$dest/"; fi
  done
  copy_font_licences "$dest"
  return
}

# Builds made with craft-fonts (CRAFT_FONTS_DIR, set by the release workflow) embed its fonts, so
# the package carries their licences as OFL-<family-dir>.txt. Nothing without CRAFT_FONTS_DIR.
copy_font_licences() {
  local dest="$1" f
  cp "$ROOT/assets/fonts/opengost/OFL.txt" "$dest/OFL-OpenGost.txt"
  [[ -n "${CRAFT_FONTS_DIR:-}" ]] || return 0
  for f in "$CRAFT_FONTS_DIR"/fonts/*/OFL.txt; do
    if [[ -f "$f" ]]; then cp "$f" "$dest/OFL-$(basename "$(dirname "$f")").txt"; fi
  done
}

# Portable SHA-256 of a file (prints just the hash).
sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null; then sha256sum "$file" | cut -d' ' -f1; else shasum -a 256 "$file" | cut -d' ' -f1; fi
  return
}
