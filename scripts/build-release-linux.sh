#!/bin/bash
# Builds ACE-Step Studio for Linux x86-64: a .deb and an AppImage with the
# pinned acestep.cpp engine on Vulkan bundled. Model weights are downloaded in
# the app.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
engine="$root/desktop/src-tauri/resources/engine"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/engine-source.json")"
server="$(sed -n 's/.*"server": "\(.*\)".*/\1/p' "$root/engines/engine-source.json")"

if [ ! -x "$engine/$server" ] || ! grep -q "$commit" "$engine/runtime.json" 2>/dev/null; then
    rm -rf "$engine"
    "$root/scripts/build-engine-runtime-linux.sh" "$engine"
fi

cd "$root"
npm --prefix app ci --no-audit --no-fund
npm --prefix desktop ci --no-audit --no-fund
cd desktop
npm exec tauri build -- --config src-tauri/tauri.linux-release.conf.json --bundles deb,appimage

find src-tauri/target/release/bundle -name '*.deb' -o -name '*.AppImage'
