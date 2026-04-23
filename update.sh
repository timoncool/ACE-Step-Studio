#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "========================================"
echo "  ACE-Step Studio - Update"
echo "========================================"

if ! command -v git &>/dev/null; then
    echo "ERROR: Git not found! https://git-scm.com/downloads"
    exit 1
fi

# ============================================================
#  Step 1: Pull latest code
# ============================================================
if [ -d ".git" ]; then
    echo ""
    echo "[1/5] Updating ACE-Step Studio..."
    git stash 2>/dev/null || true
    git pull
    git stash pop 2>/dev/null || true
else
    echo "[1/5] No git repo, skipping code update"
fi

# ============================================================
#  Step 2: Update Python deps
# ============================================================
if [ -f "venv/bin/python" ]; then
    PYTHON="$SCRIPT_DIR/venv/bin/python"
    echo ""
    echo "[2/5] Updating Python dependencies..."
    "$PYTHON" -m pip install --upgrade pip

    if [ -d "ACE-Step-1.5/acestep/third_parts/nano-vllm" ]; then
        "$PYTHON" -m pip install -e ACE-Step-1.5/acestep/third_parts/nano-vllm/
    fi

    "$PYTHON" -m pip install --upgrade \
        "transformers>=4.51.0,<4.58.0" diffusers gradio==6.2.0 \
        matplotlib scipy soundfile loguru einops accelerate fastapi diskcache \
        "uvicorn[standard]" numba vector-quantize-pytorch torchcodec \
        "torchao>=0.16.0,<0.17.0" toml peft modelscope tensorboard \
        typer-slim hf_transfer hf_xet lightning lycoris-lora safetensors xxhash

    "$PYTHON" -m pip install -e ACE-Step-1.5/ --no-deps
else
    echo "[2/5] Python venv not found, skipping. Run install.sh first!"
fi

# ============================================================
#  Step 3-5: Update npm deps + rebuild frontend
# ============================================================
if [ -f "node/bin/node" ]; then
    export PATH="$SCRIPT_DIR/node/bin:$PATH"

    echo ""
    echo "[3/5] Updating frontend dependencies..."
    if [ -f "app/package.json" ]; then
        cd "$SCRIPT_DIR/app"
        "$SCRIPT_DIR/node/bin/npm" install
        cd "$SCRIPT_DIR"
    fi

    echo ""
    echo "[4/5] Updating server dependencies..."
    if [ -f "app/server/package.json" ]; then
        cd "$SCRIPT_DIR/app/server"
        "$SCRIPT_DIR/node/bin/npm" install
        cd "$SCRIPT_DIR"
    fi

    echo ""
    echo "[5/5] Rebuilding frontend..."
    if [ -f "app/vite.config.ts" ]; then
        cd "$SCRIPT_DIR/app"
        "$SCRIPT_DIR/node/bin/npx" vite build
        cd "$SCRIPT_DIR"
    fi
else
    echo "[3-5/5] Node.js not found, skipping npm steps. Run install.sh first!"
fi

echo ""
echo "========================================"
echo "  Update complete!"
echo "========================================"
