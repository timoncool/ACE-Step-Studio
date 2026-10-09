#!/bin/bash
# macOS counterpart of build-engine-runtime.ps1: builds the pinned engine
# server engines/engine-source.json names (acestep.cpp `ace-server`) with the
# Metal backend and copies it, with its ggml libraries, into OUTPUT_DIRECTORY.
set -euo pipefail

output="${1:?usage: build-engine-runtime.sh OUTPUT_DIRECTORY}"
root="$(cd "$(dirname "$0")/.." && pwd)"
source_file="$root/engines/engine-source.json"
repository="$(sed -n 's/.*"repository": "\(.*\)".*/\1/p' "$source_file")"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$source_file")"
server="$(sed -n 's/.*"server": "\(.*\)".*/\1/p' "$source_file")"
worktree="${STUDIO_ENGINE_BUILD_ROOT:-${TMPDIR:-/tmp}}/$(sed -n 's/.*"worktree": "\(.*\)".*/\1/p' "$source_file")"

if [ ! -d "$worktree/.git" ]; then
    git clone "$repository" "$worktree"
fi
git -C "$worktree" fetch --quiet origin "$commit" || git -C "$worktree" fetch --quiet origin
git -C "$worktree" checkout --quiet "$commit"
git -C "$worktree" submodule update --init --recursive

cmake -S "$worktree" -B "$worktree/build" -DCMAKE_BUILD_TYPE=Release \
    -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON
cmake --build "$worktree/build" --config Release --target "$server" \
    -j "$(getconf _NPROCESSORS_ONLN)"

mkdir -p "$output"
cp "$worktree/build/$server" "$output/"
# -a keeps the version symlinks of the dylibs; the binary finds them via @rpath.
cp -a "$worktree"/build/*.dylib "$output/"
echo "$server ($commit) built into $output"
