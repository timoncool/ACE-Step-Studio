#!/bin/bash
set -euo pipefail

echo "========================================"
echo "  ACE-Step Studio (NO LM mode)"
echo "========================================"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# === Checks ===
if [ ! -f "venv/bin/python" ]; then
    echo "ERROR: Python venv not found! Run install.sh first"
    exit 1
fi
if [ ! -f "node/bin/node" ]; then
    echo "ERROR: Node.js not found! Run install.sh first"
    exit 1
fi
if [ ! -d "ACE-Step-1.5" ]; then
    echo "ERROR: ACE-Step-1.5 not found!"
    exit 1
fi

# === Environment isolation ===
export TMPDIR="$SCRIPT_DIR/temp"
mkdir -p "$TMPDIR"

export HF_HOME="$SCRIPT_DIR/models"
export HUGGINGFACE_HUB_CACHE="$SCRIPT_DIR/models"
export TRANSFORMERS_CACHE="$SCRIPT_DIR/models"
export HF_HUB_ENABLE_HF_TRANSFER=1
mkdir -p "$HF_HOME"

export TORCH_HOME="$SCRIPT_DIR/models/torch"
mkdir -p "$TORCH_HOME"

export XDG_CACHE_HOME="$SCRIPT_DIR/cache"
mkdir -p "$XDG_CACHE_HOME"

if [ -f "$SCRIPT_DIR/ffmpeg/ffmpeg" ]; then
    export PATH="$SCRIPT_DIR/ffmpeg:$PATH"
fi

export PYTHONIOENCODING=utf-8
export PYTHONUNBUFFERED=1
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True

export PATH="$SCRIPT_DIR/node/bin:$PATH"

# === Pipeline config ===
export PYTHON_PATH="$SCRIPT_DIR/venv/bin/python"
export ACESTEP_PATH="$SCRIPT_DIR/ACE-Step-1.5"
export DEFAULT_MODEL="marcorez8/acestep-v15-xl-turbo-bf16"
export MANAGE_PIPELINE=true
export INIT_LLM=false

if [ -f "cuda_version.txt" ]; then
    CUDA_VERSION="$(cat cuda_version.txt)"
    echo "GPU: $CUDA_VERSION"
fi

# === Install npm deps if needed ===
if [ ! -d "app/node_modules" ]; then
    echo "Installing npm dependencies..."
    cd app
    "$SCRIPT_DIR/node/bin/npm" install
    cd "$SCRIPT_DIR"
fi

# === Build frontend if dist/ missing ===
if [ ! -d "app/dist" ]; then
    echo "Building frontend..."
    cd app
    "$SCRIPT_DIR/node/bin/npx" vite build
    cd "$SCRIPT_DIR"
fi

# === Create output dirs ===
mkdir -p app/data app/server/public/audio

echo ""
echo "========================================"
echo "  NO LM mode (more VRAM for DiT)"
echo "  Express + Pipeline + Frontend"
echo "  UI: http://localhost:3001"
echo "  Press Ctrl+C to stop"
echo "========================================"
echo ""

# === Start Express (manages everything, opens browser when pipeline ready) ===
"$SCRIPT_DIR/node/bin/node" \
    "$SCRIPT_DIR/app/server/node_modules/tsx/dist/cli.mjs" \
    "$SCRIPT_DIR/app/server/src/index.ts"
