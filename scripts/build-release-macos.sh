#!/bin/bash
# Builds an unsigned (ad-hoc signed) ACE-Step Studio .dmg for Apple Silicon:
# the pinned acestep.cpp engine and the audio-to-MIDI tool, both with Metal,
# are bundled into the app. Model weights are not part of it; they are
# downloaded in the app on first start.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
engine="$root/desktop/src-tauri/resources/engine"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/engine-source.json")"
server="$(sed -n 's/.*"server": "\(.*\)".*/\1/p' "$root/engines/engine-source.json")"

if [ ! -x "$engine/$server" ] || [ "$(cat "$engine/runtime.commit" 2>/dev/null)" != "$commit" ]; then
    rm -rf "$engine"
    "$root/scripts/build-engine-runtime.sh" "$engine"
    echo "$commit" > "$engine/runtime.commit"
fi

midi="$root/desktop/src-tauri/resources/music-midi"
midi_commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/music-midi-source.json")"
if [ ! -x "$midi/music-midi" ] || [ "$(cat "$midi/runtime.commit" 2>/dev/null)" != "$midi_commit" ]; then
    rm -rf "$midi"
    "$root/scripts/build-midi-runtime.sh" "$midi"
    echo "$midi_commit" > "$midi/runtime.commit"
fi

cd "$root"
npm --prefix app install
npm --prefix desktop install
cd desktop
npm exec tauri build -- --config src-tauri/tauri.macos-release.conf.json --bundles dmg

find src-tauri/target/release/bundle/dmg -name '*.dmg'
