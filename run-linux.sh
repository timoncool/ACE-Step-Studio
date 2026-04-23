#!/bin/bash
set -euo pipefail

echo "========================================"
echo "  ACE-Step Studio (Single Terminal)"
echo "========================================"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# === Resolve Python backend ===
CONDA_ENV_NAME="acestep"
if [ -f "python_backend.txt" ]; then
    BACKEND="$(cat python_backend.txt)"
    if [[ "$BACKEND" == conda:* ]]; then
        CONDA_ENV_NAME="${BACKEND#conda:}"
    fi
fi

# === Checks ===
if ! conda env list 2>/dev/null | grep -q "^${CONDA_ENV_NAME}[[:space:]]"; then
    echo "ERROR: conda env '$CONDA_ENV_NAME' not found! Run minimal-install-linux.sh first"
    exit 1
fi

PYTHON="$(conda run -n "$CONDA_ENV_NAME" which python 2>/dev/null || true)"
if [ -z "$PYTHON" ]; then
    echo "ERROR: Python not found in conda env '$CONDA_ENV_NAME'"
    exit 1
fi

if ! command -v node &>/dev/null; then
    echo "ERROR: node not found! Install Node.js 18+ from https://nodejs.org"
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

# === Pipeline config ===
export PYTHON_PATH="$PYTHON"
export ACESTEP_PATH="$SCRIPT_DIR/ACE-Step-1.5"
export DEFAULT_MODEL="marcorez8/acestep-v15-xl-turbo-bf16"
export MANAGE_PIPELINE=true

if [ -f "cuda_version.txt" ]; then
    CUDA_VERSION="$(cat cuda_version.txt)"
    echo "GPU: $CUDA_VERSION"
fi

# === Install npm deps if needed ===
if [ ! -d "app/node_modules" ]; then
    echo "Installing npm dependencies..."
    npm install --prefix "$SCRIPT_DIR/app"
fi

if [ ! -d "app/server/node_modules" ]; then
    echo "Installing server npm dependencies..."
    npm install --prefix "$SCRIPT_DIR/app/server"
fi

# === Build frontend if dist/ missing ===
if [ ! -d "app/dist" ]; then
    echo "Building frontend..."
    npx --prefix "$SCRIPT_DIR/app" vite build
fi

# === Create output dirs ===
mkdir -p app/data app/server/public/audio

echo ""
echo "========================================"
echo "  Single terminal mode"
echo "  Express + Pipeline + Frontend"
echo "  UI: http://localhost:3001"
echo "  Press Ctrl+C to stop"
echo "========================================"
echo ""

# === Start Express (manages everything, opens browser when pipeline ready) ===
node \
    "$SCRIPT_DIR/app/server/node_modules/tsx/dist/cli.mjs" \
    "$SCRIPT_DIR/app/server/src/index.ts"
