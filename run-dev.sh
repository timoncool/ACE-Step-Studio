#!/bin/bash
set -euo pipefail

echo "========================================"
echo "  ACE-Step Studio"
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

# === Default model ===
export DEFAULT_MODEL="acestep-v15-xl-turbo"
export ACESTEP_API_URL="http://localhost:8001"
export ACESTEP_PATH="$SCRIPT_DIR/ACE-Step-1.5"
export PYTHON_PATH="$SCRIPT_DIR/venv/bin/python"

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

# === Create output dirs ===
mkdir -p app/data app/server/public/audio

echo ""
echo "Starting 3 services:"
echo "  [1] Gradio pipeline (port 8001)"
echo "  [2] Express backend (port 3001)"
echo "  [3] Vite frontend  (port 3000)"
echo ""

# === Cleanup on exit ===
GRADIO_PID=""
EXPRESS_PID=""

cleanup() {
    echo ""
    echo "Shutting down..."
    [ -n "$GRADIO_PID" ]  && kill "$GRADIO_PID"  2>/dev/null || true
    [ -n "$EXPRESS_PID" ] && kill "$EXPRESS_PID" 2>/dev/null || true
    echo "Done."
}
trap cleanup EXIT INT TERM

# === Start Gradio pipeline ===
echo "Starting Gradio pipeline with $DEFAULT_MODEL..."
cd "$SCRIPT_DIR/ACE-Step-1.5"
"$SCRIPT_DIR/venv/bin/python" -m acestep.acestep_v15_pipeline \
    --config_path "$DEFAULT_MODEL" \
    --port 8001 \
    --init_service true \
    --init_llm true &
GRADIO_PID=$!

echo "Waiting for Gradio to initialize..."
sleep 5

# === Start Express backend ===
echo "Starting Express backend..."
cd "$SCRIPT_DIR/app/server"
"$SCRIPT_DIR/node/bin/node" \
    "$SCRIPT_DIR/app/server/node_modules/tsx/dist/cli.mjs" \
    src/index.ts &
EXPRESS_PID=$!

sleep 2

# === Start Vite frontend (foreground) ===
echo ""
echo "========================================"
echo "  UI will open at http://localhost:3000"
echo "  Press Ctrl+C to stop all services"
echo "========================================"
echo ""

cd "$SCRIPT_DIR/app"
"$SCRIPT_DIR/node/bin/npx" vite --open
