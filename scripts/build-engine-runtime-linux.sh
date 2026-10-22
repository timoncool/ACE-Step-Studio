#!/bin/bash
# Linux counterpart of build-engine-runtime.ps1: builds the pinned engine
# server engines/engine-source.json names (acestep.cpp `ace-server`) with the
# Vulkan backend, the CUDA 13 backend when nvcc is present, and a build of
# ggml-cpu for every processor generation, each loaded as a library, and
# copies it with its libraries into OUTPUT_DIRECTORY.
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

# configured from nothing each time (Ubuntu 22.04 CMake has no --fresh)
rm -rf "$worktree/build-linux"
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

# The CUDA backend, when this machine has the CUDA 13 toolkit: ggml-cuda alone, loaded by the engine from cuda13/ when
# the studio names it (STUDIO_CUDA_BACKEND), device code for every architecture CUDA 13 targets, no PTX. cuBLAS goes
# beside the engine, where the backend's RUNPATH ($ORIGIN/..) finds it.
backend="vulkan"
if command -v nvcc >/dev/null 2>&1; then
    cuda_root="$(dirname "$(dirname "$(command -v nvcc)")")"
    rm -rf "$worktree/build-linux-cuda13"
    cmake -S "$worktree" -B "$worktree/build-linux-cuda13" -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DGGML_CUDA_FA_ALL_QUANTS=ON \
        "-DCMAKE_CUDA_ARCHITECTURES=75-real;80-real;86-real;89-real;90-real;120a-real" \
        -DCMAKE_BUILD_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_INSTALL_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
    cmake --build "$worktree/build-linux-cuda13" --config Release --target ggml-cuda -j "$(getconf _NPROCESSORS_ONLN)"
    cuda="$(find "$worktree/build-linux-cuda13" -name 'libggml-cuda.so' -type f | head -n 1)"
    [ -n "$cuda" ] || { echo "[ERROR] the CUDA build completed without libggml-cuda.so" >&2; exit 1; }
    mkdir -p "$output/cuda13"
    cp "$cuda" "$output/cuda13/"
    # the CUDA libraries it names, by the loader's names (.so.13), with the ones those name in turn (cuBLAS -> cuBLASLt)
    cuda_lib="$cuda_root/targets/x86_64-linux/lib"
    [ -d "$cuda_lib" ] || cuda_lib="$cuda_root/lib64"
    wanted="$(readelf -d "$cuda" | sed -n 's/.*NEEDED.*\[\(libcu[A-Za-z]*\.so\.[0-9]*\)\].*/\1/p')"
    while [ -n "$wanted" ]; do
        next=""
        for library in $wanted; do
            # libcuda.so.1 is the display driver's, never shipped
            case "$library" in libcuda.so.*) continue ;; esac
            [ -e "$output/$library" ] && continue
            [ -e "$cuda_lib/$library" ] || { echo "[ERROR] $library is not in $cuda_lib" >&2; exit 1; }
            cp -L "$cuda_lib/$library" "$output/$library"
            next="$next $(readelf -d "$output/$library" | sed -n 's/.*NEEDED.*\[\(libcu[A-Za-z]*\.so\.[0-9]*\)\].*/\1/p')"
        done
        wanted="$next"
    done
    backend="cuda13,vulkan"
fi
printf '{"commit":"%s","backend":"%s","platform":"linux"}\n' "$commit" "$backend" > "$output/runtime.json"
readelf -d "$output/$server" | grep -iE 'runpath|rpath' || true
echo "$server ($commit) built into $output"
