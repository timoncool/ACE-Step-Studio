#!/bin/bash
# Linux counterpart of build-engine-runtime.ps1: builds the pinned engine
# server engines/engine-source.json names (acestep.cpp `ace-server`) with the
# Vulkan backend and a build of ggml-cpu for every processor generation, each
# loaded as a library, and copies it with its libraries into OUTPUT_DIRECTORY.
# The libraries are found beside the binary ($ORIGIN), wherever the package
# puts them.
set -euo pipefail

output="${1:?usage: build-engine-runtime-linux.sh OUTPUT_DIRECTORY}"
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

cmake -S "$worktree" -B "$worktree/build-linux" -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DGGML_NATIVE=OFF -DGGML_VULKAN=ON -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON \
    -DCMAKE_BUILD_RPATH='$ORIGIN' -DCMAKE_INSTALL_RPATH='$ORIGIN' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
# the backends are modules nothing links against, so each is a target of its own
variants="$(sed -n 's/^build \(ggml-cpu-[a-z0-9_]*\): phony.*/\1/p' "$worktree/build-linux/build.ninja" | sort -u | tr '\n' ' ')"
[ -n "$variants" ] || { echo "[ERROR] the configured build has no processor variants of ggml-cpu" >&2; exit 1; }
targets="$server ggml-vulkan $variants"
cmake --build "$worktree/build-linux" --config Release --target $targets -j "$(getconf _NPROCESSORS_ONLN)"

mkdir -p "$output"
binary="$(find "$worktree/build-linux" -maxdepth 2 -type f -name "$server" | head -n 1)"
[ -n "$binary" ] || { echo "[ERROR] the build completed without $server" >&2; exit 1; }
cp "$binary" "$output/"
# -a keeps the version symlinks; the binary finds them through $ORIGIN.
find "$worktree/build-linux" -maxdepth 2 -name 'lib*.so*' -exec cp -a {} "$output/" \;
printf '{"commit":"%s","backend":"vulkan","platform":"linux"}\n' "$commit" > "$output/runtime.json"
readelf -d "$output/$server" | grep -iE 'runpath|rpath' || true
echo "$server ($commit) built into $output"
