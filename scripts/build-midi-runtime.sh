#!/bin/bash
# macOS counterpart of the Windows music-midi.exe: builds HOT-Step's `ace-midi`
# (MuScriptor audio-to-MIDI) at the pinned commit with Metal, and copies it as
# `music-midi`, with its ggml libraries, into OUTPUT_DIRECTORY.
set -euo pipefail

output="${1:?usage: build-midi-runtime.sh OUTPUT_DIRECTORY}"
root="$(cd "$(dirname "$0")/.." && pwd)"
source_file="$root/engines/music-midi-source.json"
repository="$(sed -n 's/.*"repository": "\(.*\)".*/\1/p' "$source_file")"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$source_file")"
worktree="${STUDIO_ENGINE_BUILD_ROOT:-${TMPDIR:-/tmp}}/hot-step-engine"

if [ ! -d "$worktree/.git" ]; then
    git clone "$repository" "$worktree"
fi
git -C "$worktree" fetch --quiet origin "$commit" || git -C "$worktree" fetch --quiet origin
git -C "$worktree" checkout --quiet "$commit"
git -C "$worktree" submodule update --init --recursive

cmake -S "$worktree/engine" -B "$worktree/engine/build" -DCMAKE_BUILD_TYPE=Release \
    -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON
cmake --build "$worktree/engine/build" --config Release --target ace-midi \
    -j "$(getconf _NPROCESSORS_ONLN)"

mkdir -p "$output"
cp "$worktree/engine/build/ace-midi" "$output/music-midi"
cp -a "$worktree"/engine/build/*.dylib "$output/"
echo "music-midi ($commit) built into $output"
