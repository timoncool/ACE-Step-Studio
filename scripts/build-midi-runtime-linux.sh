#!/bin/bash
# Linux counterpart of the Windows music-midi.exe: builds HOT-Step's `ace-midi`
# (MuScriptor audio-to-MIDI) at the pinned commit with a build of ggml-cpu for
# every processor generation, each loaded as a library, and copies it as
# `music-midi`, with its libraries, into OUTPUT_DIRECTORY. The studio runs it on
# the processor here: its Vulkan path writes wrong notes. The libraries are found
# beside the binary ($ORIGIN), wherever the package puts them.
set -euo pipefail

output="${1:?usage: build-midi-runtime-linux.sh OUTPUT_DIRECTORY}"
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
# ggml whole; of the VST3 SDK only what the engine's hosting library compiles -
# its documentation and examples are submodules that do not always resolve
git -C "$worktree" submodule update --init --recursive engine/ggml
git -C "$worktree" submodule update --init engine/vendor/vst3sdk
git -C "$worktree/engine/vendor/vst3sdk" submodule update --init base pluginterfaces public.sdk

build="$worktree/engine/build-linux"
cmake -S "$worktree/engine" -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DHOT_STEP_ORT_PATHS=OFF
# the backends are modules nothing links against, so each is a target of its own
variants="$(sed -n 's/^build \(ggml-cpu-[a-z0-9_]*\): phony.*/\1/p' "$build/build.ninja" | sort -u | tr '\n' ' ')"
[ -n "$variants" ] || { echo "[ERROR] the configured build has no processor variants of ggml-cpu" >&2; exit 1; }
cmake --build "$build" --config Release --target ace-midi $variants -j "$(getconf _NPROCESSORS_ONLN)"

mkdir -p "$output"
cp "$build/ace-midi" "$output/music-midi"
# -a keeps the version symlinks; the binary finds them through $ORIGIN.
find "$build" -maxdepth 2 -name 'lib*.so*' -exec cp -a {} "$output/" \;
echo "$commit" > "$output/runtime.commit"
if readelf -d "$output/music-midi" | grep -iE 'runpath|rpath' | grep -qv 'ORIGIN'; then
    echo "[ERROR] music-midi keeps a build-machine library path:" >&2
    readelf -d "$output/music-midi" | grep -iE 'runpath|rpath' >&2
    exit 1
fi
echo "music-midi ($commit) built into $output"
