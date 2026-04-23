#!/bin/bash
set -euo pipefail

echo "========================================"
echo "  ACE-Step Studio - Install"
echo "========================================"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export TMPDIR="$SCRIPT_DIR/temp"
mkdir -p downloads temp models cache app/data app/server/public/audio

OS="$(uname -s)"
ARCH="$(uname -m)"

# ============================================================
#  Step 1: GPU / compute backend selection
# ============================================================
echo ""

if [ "$OS" = "Darwin" ]; then
    echo "Select your compute backend:"
    echo ""
    echo "  1. Apple Silicon (MPS)"
    echo "  2. CPU only"
    echo ""
    read -rp "Enter number (1-2): " GPU_CHOICE
    case "$GPU_CHOICE" in
        1) CUDA_VERSION="mps";  CUDA_NAME="Apple Silicon (MPS)";;
        2) CUDA_VERSION="cpu";  CUDA_NAME="CPU only";;
        *) echo "Invalid choice!"; exit 1;;
    esac
else
    echo "Select your GPU:"
    echo ""
    echo "  1. NVIDIA GTX 10xx (Pascal)"
    echo "  2. NVIDIA RTX 20xx (Turing)"
    echo "  3. NVIDIA RTX 30xx (Ampere)"
    echo "  4. NVIDIA RTX 40xx (Ada Lovelace)"
    echo "  5. NVIDIA RTX 50xx (Blackwell)"
    echo "  6. CPU only (no GPU)"
    echo ""
    read -rp "Enter number (1-6): " GPU_CHOICE
    case "$GPU_CHOICE" in
        1) CUDA_VERSION="cu118"; CUDA_NAME="CUDA 11.8 (GTX 10xx)";;
        2) CUDA_VERSION="cu126"; CUDA_NAME="CUDA 12.6 (RTX 20xx)";;
        3) CUDA_VERSION="cu126"; CUDA_NAME="CUDA 12.6 (RTX 30xx)";;
        4) CUDA_VERSION="cu128"; CUDA_NAME="CUDA 12.8 (RTX 40xx)";;
        5) CUDA_VERSION="cu128"; CUDA_NAME="CUDA 12.8 (RTX 50xx)";;
        6) CUDA_VERSION="cpu";   CUDA_NAME="CPU only";;
        *) echo "Invalid choice!"; exit 1;;
    esac
fi

TORCH_VERSION="2.7.1"
TORCHAUDIO_VERSION="2.7.1"

echo ""
echo "Selected: $CUDA_NAME"
echo ""

# ============================================================
#  Step 2: Python virtual environment
# ============================================================
if [ -f "venv/bin/python" ]; then
    echo "[OK] Python venv already exists"
else
    echo "[1/7] Setting up Python virtual environment..."
    PYTHON_CMD=""
    for py in python3.12 python3.11 python3.10 python3; do
        if command -v "$py" &>/dev/null; then
            PYTHON_CMD="$py"
            break
        fi
    done
    if [ -z "$PYTHON_CMD" ]; then
        echo "ERROR: Python 3.10+ not found! Install Python 3.12 first."
        echo "  Linux:  sudo apt install python3.12 python3.12-venv"
        echo "  Mac:    brew install python@3.12"
        exit 1
    fi
    "$PYTHON_CMD" -m venv venv
    echo "[OK] Python venv created using $PYTHON_CMD"
fi

PYTHON="$SCRIPT_DIR/venv/bin/python"

# ============================================================
#  Step 3: pip
# ============================================================
echo "[2/7] Upgrading pip..."
"$PYTHON" -m pip install --upgrade pip --quiet

# ============================================================
#  Step 4: PyTorch
# ============================================================
echo "[3/7] Installing PyTorch $TORCH_VERSION ($CUDA_NAME)..."
if [ "$OS" = "Darwin" ]; then
    "$PYTHON" -m pip install \
        torch==$TORCH_VERSION torchaudio==$TORCHAUDIO_VERSION torchvision
else
    "$PYTHON" -m pip install \
        torch==$TORCH_VERSION torchaudio==$TORCHAUDIO_VERSION torchvision \
        --index-url "https://download.pytorch.org/whl/$CUDA_VERSION"
fi

# ============================================================
#  Step 5: ACE-Step dependencies
# ============================================================
echo "[4/7] Installing ACE-Step dependencies..."
"$PYTHON" -m pip install hatchling editables

"$PYTHON" -m pip install \
    -e ACE-Step-1.5/acestep/third_parts/nano-vllm/

"$PYTHON" -m pip install \
    "transformers>=4.51.0,<4.58.0" diffusers gradio==6.2.0 \
    matplotlib scipy soundfile loguru einops accelerate fastapi diskcache \
    "uvicorn[standard]" numba vector-quantize-pytorch torchcodec \
    "torchao>=0.16.0,<0.17.0" toml peft modelscope tensorboard \
    typer-slim hf_transfer hf_xet lightning lycoris-lora safetensors xxhash

if [ "$CUDA_VERSION" != "cpu" ] && [ "$CUDA_VERSION" != "mps" ]; then
    echo "Installing Triton for torch.compile..."
    "$PYTHON" -m pip install "triton>=3.0.0,<3.4"
fi

if [ "$CUDA_VERSION" = "cu128" ] && [ "$OS" != "Darwin" ]; then
    echo "Installing Flash Attention 2..."
    "$PYTHON" -m pip install flash-attn --no-build-isolation || \
        echo "WARNING: Flash Attention failed to install. Continuing without it."
fi

"$PYTHON" -m pip install -e ACE-Step-1.5/ --no-deps

# ============================================================
#  Step 6: Node.js
# ============================================================
NODE_VERSION="22.18.0"

if [ -f "node/bin/node" ]; then
    echo "[OK] Node.js already installed"
else
    echo "[5/7] Downloading Node.js 22 LTS..."
    mkdir -p node

    if [ "$OS" = "Darwin" ]; then
        if [ "$ARCH" = "arm64" ]; then
            NODE_URL="https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-darwin-arm64.tar.gz"
        else
            NODE_URL="https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-darwin-x64.tar.gz"
        fi
        curl -fL "$NODE_URL" -o downloads/node.tar.gz
        tar -xzf downloads/node.tar.gz -C node/ --strip-components=1
    else
        NODE_URL="https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz"
        curl -fL "$NODE_URL" -o downloads/node.tar.xz
        tar -xJf downloads/node.tar.xz -C node/ --strip-components=1
    fi
    echo "[OK] Node.js 22 LTS installed"
fi

export PATH="$SCRIPT_DIR/node/bin:$PATH"

# ============================================================
#  Step 7: npm dependencies
# ============================================================
echo "[6/7] Installing npm dependencies..."

echo "  Installing frontend deps..."
cd "$SCRIPT_DIR/app"
"$SCRIPT_DIR/node/bin/npm" install

echo "  Installing server deps..."
cd "$SCRIPT_DIR/app/server"
"$SCRIPT_DIR/node/bin/npm" install

# ============================================================
#  Step 8: Build frontend
# ============================================================
echo "[7/7] Building frontend..."
cd "$SCRIPT_DIR/app"
"$SCRIPT_DIR/node/bin/npx" vite build

# ============================================================
#  Step 9: FFmpeg
# ============================================================
cd "$SCRIPT_DIR"

if [ -f "ffmpeg/ffmpeg" ]; then
    echo "[OK] FFmpeg already installed"
elif command -v ffmpeg &>/dev/null; then
    echo "[OK] FFmpeg found in system PATH"
else
    echo "Downloading FFmpeg..."
    mkdir -p ffmpeg

    if [ "$OS" = "Darwin" ]; then
        if [ "$ARCH" = "arm64" ]; then
            FFMPEG_URL="https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip"
        else
            FFMPEG_URL="https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip"
        fi
        curl -fL "$FFMPEG_URL" -o downloads/ffmpeg.zip
        unzip -q downloads/ffmpeg.zip -d ffmpeg/
        chmod +x ffmpeg/ffmpeg
    else
        FFMPEG_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"
        curl -fL "$FFMPEG_URL" -o downloads/ffmpeg.tar.xz
        mkdir -p downloads/ffmpeg-extract
        tar -xJf downloads/ffmpeg.tar.xz -C downloads/ffmpeg-extract/ --strip-components=1
        cp downloads/ffmpeg-extract/bin/ffmpeg ffmpeg/ffmpeg
        cp downloads/ffmpeg-extract/bin/ffprobe ffmpeg/ffprobe
        rm -rf downloads/ffmpeg-extract
        chmod +x ffmpeg/ffmpeg ffmpeg/ffprobe
    fi
    echo "[OK] FFmpeg installed"
fi

# ============================================================
#  Save GPU config
# ============================================================
echo "$CUDA_VERSION" > cuda_version.txt

echo ""
echo "========================================"
echo "  Installation complete!"
echo ""
echo "  To start: ./run.sh"
echo "  Models download automatically on first run."
echo "========================================"
