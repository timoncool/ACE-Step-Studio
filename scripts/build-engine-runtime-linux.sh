#!/bin/bash
# Linux counterpart of build-engine-runtime.ps1: builds the pinned engine
# server engines/engine-source.json names (acestep.cpp `ace-server`) with the
# Vulkan backend, the CUDA 13 and CUDA 12 backends where their toolkits are, and a build of
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

# The CUDA backends, each when this machine has its toolkit (CUDA13_ROOT, CUDA12_ROOT): ggml-cuda alone, loaded by the
# engine from cuda13/ or cuda12/ when the studio names it (STUDIO_CUDA_BACKEND), device code for every architecture of
# that CUDA, no PTX. Their CUDA runtime and cuBLAS are not shipped: the studio downloads NVIDIA's wheels into one folder
# the engine and the audio.cpp server share, and names it in LD_LIBRARY_PATH.
build_cuda() {
    local name="$1" root="$2" architectures="$3"
    rm -rf "$worktree/build-linux-$name"
    cmake -S "$worktree" -B "$worktree/build-linux-$name" -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DGGML_CUDA_FA_ALL_QUANTS=ON \
        "-DCMAKE_CUDA_ARCHITECTURES=$architectures" "-DCMAKE_CUDA_COMPILER=$root/bin/nvcc" "-DCUDAToolkit_ROOT=$root" \
        -DCMAKE_CUDA_FLAGS=-Wno-deprecated-gpu-targets \
        -DCMAKE_BUILD_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_INSTALL_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
    cmake --build "$worktree/build-linux-$name" --config Release --target ggml-cuda -j "$(getconf _NPROCESSORS_ONLN)"
    local cuda
    cuda="$(find "$worktree/build-linux-$name" -name 'libggml-cuda.so' -type f | head -n 1)"
    [ -n "$cuda" ] || { echo "[ERROR] the $name build completed without libggml-cuda.so" >&2; exit 1; }
    mkdir -p "$output/$name"
    cp "$cuda" "$output/$name/"
    # CMake appends the toolkit's library folder of this machine to the RUNPATH
    patchelf --set-rpath '$ORIGIN:$ORIGIN/..' "$output/$name/libggml-cuda.so"
}
backend="vulkan"
if [ -x "${CUDA12_ROOT:-}/bin/nvcc" ]; then
    build_cuda cuda12 "$CUDA12_ROOT" "52-real;60-real;61-real;70-real;75-real;80-real;86-real;89-real;90-real;120a-real"
    backend="cuda12,$backend"
fi
if [ -x "${CUDA13_ROOT:-}/bin/nvcc" ]; then
    build_cuda cuda13 "$CUDA13_ROOT" "75-real;80-real;86-real;89-real;90-real;120a-real"
    backend="cuda13,$backend"
fi
printf '{"commit":"%s","backend":"%s","platform":"linux"}\n' "$commit" "$backend" > "$output/runtime.json"
for binary in "$output/$server" "$output"/*.so* "$output"/cuda13/*.so* "$output"/cuda12/*.so*; do
    [ -f "$binary" ] || continue
    if readelf -d "$binary" | grep -iE 'runpath|rpath' | grep -qE '(\[|:)/'; then
        echo "[ERROR] $binary keeps an absolute RUNPATH:" >&2
        readelf -d "$binary" | grep -iE 'runpath|rpath' >&2
        exit 1
    fi
done
echo "$server ($commit) built into $output"
