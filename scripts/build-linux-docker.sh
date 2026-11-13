#!/bin/bash
# The Linux x86-64 .deb and AppImage, built on this machine in an Ubuntu 22.04 container (glibc 2.35, so they run there and on anything newer; Docker under WSL on
# Windows) from the committed HEAD. Only the .dmg is built on GitHub (release-unix.yml): there is no Mac here.
#   scripts/build-linux-docker.sh <output folder> [cache folder]
# The cache keeps Rust, Node, the cargo target and the engine build between runs.
set -euo pipefail

if [ "${1:-}" = "--inside" ]; then
  export DEBIAN_FRONTEND=noninteractive APPIMAGE_EXTRACT_AND_RUN=1 RUSTUP_TOOLCHAIN=stable CARGO_TERM_COLOR=never
  export CARGO_HOME=/cache/cargo RUSTUP_HOME=/cache/rustup npm_config_cache=/cache/npm PATH=/cache/cargo/bin:/cache/node/bin:$PATH
  echo "[..] packages"
  apt-get update -q >/dev/null
  apt-get install -y -q --no-install-recommends build-essential pkg-config cmake ninja-build nasm autoconf automake libtool \
    curl ca-certificates git unzip xz-utils libssl-dev libasound2-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
    librsvg2-dev patchelf file libfuse2 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav libvulkan-dev >/dev/null
  if [ ! -x /cache/node/bin/node ]; then
    echo "[..] node 22"
    v=$(curl -s https://nodejs.org/dist/latest-v22.x/ | grep -o 'node-v22[0-9.]*-linux-x64.tar.xz' | head -1)
    mkdir -p /cache/node && curl -sL "https://nodejs.org/dist/latest-v22.x/$v" | tar -xJ -C /cache/node --strip-components=1
  fi
  if [ ! -x /cache/cargo/bin/cargo ]; then
    echo "[..] rust stable"
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
  fi
  # Ubuntu 22.04 has no glslc and older Vulkan headers than the engine's ggml wants: LunarG's SDK has both
  vulkan=1.4.363.0
  if [ ! -x /cache/vulkansdk-$vulkan/x86_64/bin/glslc ]; then
    echo "[..] Vulkan SDK $vulkan"
    mkdir -p /cache/vulkansdk-$vulkan
    curl -sSfL "https://sdk.lunarg.com/sdk/download/$vulkan/linux/vulkansdk-linux-x86_64-$vulkan.tar.xz" | tar -xJ -C /cache/vulkansdk-$vulkan --strip-components=1
  fi
  export VULKAN_SDK=/cache/vulkansdk-$vulkan/x86_64 PATH=/cache/vulkansdk-$vulkan/x86_64/bin:$PATH
  # the engine's CUDA 13 and CUDA 12 backends: nvcc, the runtime and cuBLAS of CUDA 13.3 and of CUDA 12.9 (the toolkit
  # of nvidia/cuda:12.9.1-devel-ubuntu22.04, which builds audio.cpp's CUDA 12 server) from NVIDIA's Ubuntu 22.04
  # repository, the packages kept in the cache
  echo "[..] CUDA 13.3 and 12.9"
  curl -sSfL -o /tmp/cuda-keyring.deb https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/x86_64/cuda-keyring_1.1-1_all.deb
  dpkg -i /tmp/cuda-keyring.deb >/dev/null
  mkdir -p /cache/apt
  apt-get update -q >/dev/null
  for cuda in 13-3 12-9; do
    apt-get install -y -q --no-install-recommends -o Dir::Cache::Archives=/cache/apt cuda-nvcc-$cuda cuda-cudart-dev-$cuda libcublas-dev-$cuda >/dev/null
  done
  export CUDA13_ROOT=/usr/local/cuda-13.3 CUDA12_ROOT=/usr/local/cuda-12.9
  echo "[..] source $(git -c safe.directory=/src -C /src rev-parse --short HEAD)"
  rm -rf /cache/src && mkdir -p /cache/src
  git -c safe.directory=/src -C /src archive HEAD | tar -x -C /cache/src
  cd /cache/src
  export CARGO_TARGET_DIR=/cache/target
  ln -sfn /cache/target desktop/src-tauri/target
  echo "[..] build (log: build.log)"
  bash scripts/build-release-linux.sh > /out/build.log 2>&1 || { echo "[ERROR] build failed"; tail -40 /out/build.log; exit 1; }
  engine=desktop/src-tauri/resources/engine/ace-server
  if ldd "$engine" | grep -i 'not found'; then echo "[ERROR] the engine misses libraries"; exit 1; fi
  # libcuda.so.1 comes with the display driver, which a build container has not got
  # the CUDA runtime and cuBLAS come from the toolkits here, from NVIDIA's wheels on the user's machine
  for build in cuda13:$CUDA13_ROOT cuda12:$CUDA12_ROOT; do
    backend=desktop/src-tauri/resources/engine/${build%%:*}/libggml-cuda.so
    [ -f "$backend" ] || { echo "[ERROR] the engine was built without its ${build%%:*} backend"; exit 1; }
    if LD_LIBRARY_PATH="${build#*:}/targets/x86_64-linux/lib" ldd "$backend" | grep -i 'not found' | grep -v 'libcuda.so'; then
      echo "[ERROR] the ${build%%:*} backend misses libraries"; exit 1
    fi
  done
  if ls desktop/src-tauri/resources/engine/libcu* >/dev/null 2>&1; then echo "[ERROR] CUDA libraries are packed beside the engine"; exit 1; fi
  # the engine answers --help with its usage and a non-zero code
  "$engine" --help > /tmp/engine-help.txt 2>&1 || true
  grep -q 'Usage' /tmp/engine-help.txt || { echo "[ERROR] the engine does not start"; head -5 /tmp/engine-help.txt; exit 1; }
  caption=desktop/src-tauri/resources/music-midi/music-caption
  if ldd "$caption" | grep -i 'not found'; then echo "[ERROR] the captioner misses libraries"; exit 1; fi
  version="$(node -p "require('./desktop/src-tauri/tauri.conf.json').version")"
  product="$(node -p "require('./desktop/src-tauri/tauri.conf.json').productName")"
  # the cache may be shared with the other studios and keeps earlier versions: take this program's and this version's by full name
  deb="/cache/target/release/bundle/deb/${product}_${version}_amd64.deb"
  appimage="/cache/target/release/bundle/appimage/${product}_${version}_amd64.AppImage"
  [ -f "$deb" ] && [ -f "$appimage" ] || { echo "[ERROR] no ${product} ${version} package in the bundle folder"; ls -la /cache/target/release/bundle/deb /cache/target/release/bundle/appimage; exit 1; }
  # tauri-bundler names the package by heck::AsKebabCase(productName) and the menu entry and resource folder by productName
  package="$(node -p "process.argv[1].replace(/([a-z0-9])([A-Z])/g,'\$1 \$2').replace(/([A-Z]+)([A-Z][a-z])/g,'\$1 \$2').split(/[^A-Za-z0-9]+/).filter(Boolean).join('-').toLowerCase()" "$product")"
  found="$(dpkg-deb -f "$deb" Package)"
  [ "$found" = "$package" ] || { echo "[ERROR] $deb is the package $found, not $package"; exit 1; }
  listing="$(dpkg-deb -c "$deb")"
  grep -qF " usr/share/applications/${product}.desktop" <<< "$listing" || { echo "[ERROR] $deb has no ${product}.desktop"; exit 1; }
  grep -qF " usr/lib/${product}/resources/engine/ace-server" <<< "$listing" || { echo "[ERROR] $deb carries no ACE-Step engine"; exit 1; }
  rm -rf /tmp/appimage-check && mkdir -p /tmp/appimage-check
  # the .desktop at the image's root is a link into usr/share/applications: take the file it points to
  (cd /tmp/appimage-check && env -u APPIMAGE_EXTRACT_AND_RUN "$appimage" --appimage-extract 'usr/share/applications/*.desktop' >/dev/null)
  grep -qx "Name=${product}" /tmp/appimage-check/squashfs-root/usr/share/applications/*.desktop || { echo "[ERROR] $appimage is not ${product}"; exit 1; }
  echo "[OK] packages are ${product}: deb ${found}, AppImage $(basename /tmp/appimage-check/squashfs-root/usr/share/applications/*.desktop)"
  cp "$deb" "/out/ACE-Step-Studio-${version}-linux-amd64.deb"
  cp "$appimage" "/out/ACE-Step-Studio-${version}-linux-x86_64.AppImage"
  ls -la /out/*.deb /out/*.AppImage
  echo "[OK] ACE-Step-Studio ${version} for Linux"
  exit 0
fi

out="${1:?output folder}"
cache="${2:-$HOME/ace-linux2204-cache}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$out" "$cache"
cp "$repo/scripts/build-linux-docker.sh" "$out/.build-linux-docker.sh"
docker run --rm -v "$repo:/src:ro" -v "$(cd "$out" && pwd):/out" -v "$cache:/cache" ubuntu:22.04 bash /out/.build-linux-docker.sh --inside
