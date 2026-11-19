#!/usr/bin/env bash
# Regenerate original CADCraftGOST artwork on any platform, without sips/iconutil.
# Source artwork: xtask/src/gost_icons.rs (MIT OR Apache-2.0).
# Generated files reuse the desktop embed paths; hicolor filenames use the fork's product ID.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
cargo xtask gost-icons
