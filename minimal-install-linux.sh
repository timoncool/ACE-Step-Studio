#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==========================================="
echo "  ACE-Step Studio - Minimal Linux Install"
echo "==========================================="
echo ""

# ================================================================
#  Phase 1: Dependency checks (collect ALL issues before failing)
# ================================================================
ERRORS=()
WARNINGS=()

# --- CUDA drivers ---
if command -v nvidia-smi &>/dev/null; then
    CUDA_DRIVER="$(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1 || true)"
    CUDA_VERSION_DETECTED="$(nvidia-smi | grep -oP 'CUDA Version: \K[0-9.]+' 2>/dev/null || true)"
    echo "[OK] NVIDIA driver: $CUDA_DRIVER  (CUDA $CUDA_VERSION_DETECTED)"
else
    WARNINGS+=("nvidia-smi not found - GPU acceleration unavailable. PyTorch will run on CPU.")
    CUDA_VERSION_DETECTED=""
fi

# --- conda ---
CONDA_CMD=""
if command -v conda &>/dev/null; then
    CONDA_CMD="conda"
    echo "[OK] conda: $(conda --version)"
elif command -v mamba &>/dev/null; then
    CONDA_CMD="mamba"
    echo "[OK] mamba: $(mamba --version | head -1)"
else
    ERRORS+=("conda/mamba not found. Install Miniconda: https://docs.conda.io/en/latest/miniconda.html")
fi

# --- Python (via conda env or system) ---
PYTHON=""
CONDA_ENV_NAME="acestep"
if [ -n "$CONDA_CMD" ]; then
    if conda env list 2>/dev/null | grep -q "^${CONDA_ENV_NAME}[[:space:]]"; then
        PYTHON="$(conda run -n "$CONDA_ENV_NAME" which python 2>/dev/null || true)"
        if [ -n "$PYTHON" ]; then
            PY_VER="$("$PYTHON" --version 2>&1)"
            echo "[OK] conda env '$CONDA_ENV_NAME': $PY_VER"
        fi
    fi
fi
if [ -z "$PYTHON" ]; then
    for py in python3.12 python3.11 python3.10; do
        if command -v "$py" &>/dev/null; then
            PYTHON="$(command -v "$py")"
            echo "[OK] System Python: $("$PYTHON" --version)"
            break
        fi
    done
fi
if [ -z "$PYTHON" ]; then
    ERRORS+=("Python 3.10+ not found. Run: conda create -n acestep python=3.12")
fi

# --- pip ---
if [ -n "$PYTHON" ] && ! "$PYTHON" -m pip --version &>/dev/null 2>&1; then
    ERRORS+=("pip not available for $PYTHON. Run: $PYTHON -m ensurepip")
fi

# --- Node.js ---
NODE_MIN=18
if command -v node &>/dev/null; then
    NODE_VER_FULL="$(node --version)"
    NODE_MAJOR="${NODE_VER_FULL#v}"
    NODE_MAJOR="${NODE_MAJOR%%.*}"
    if [ "$NODE_MAJOR" -ge "$NODE_MIN" ]; then
        echo "[OK] node: $NODE_VER_FULL"
    else
        ERRORS+=("Node.js $NODE_VER_FULL is too old (need v${NODE_MIN}+). Install: https://nodejs.org/en/download")
    fi
else
    ERRORS+=("node not found. Install: https://nodejs.org/en/download  or  sudo apt install nodejs npm")
fi

# --- npm ---
if command -v npm &>/dev/null; then
    echo "[OK] npm: $(npm --version)"
else
    ERRORS+=("npm not found. Install alongside Node.js.")
fi

# --- ffmpeg ---
if command -v ffmpeg &>/dev/null; then
    FFMPEG_VER="$(ffmpeg -version 2>&1 | head -1 | grep -oP 'ffmpeg version \K\S+')"
    echo "[OK] ffmpeg: $FFMPEG_VER"
else
    WARNINGS+=("ffmpeg not found - video rendering will not work. Fix: sudo apt install ffmpeg")
fi

# --- git ---
if command -v git &>/dev/null; then
    echo "[OK] git: $(git --version)"
else
    ERRORS+=("git not found. Fix: sudo apt install git")
fi

# --- ACE-Step source ---
if [ ! -d "ACE-Step-1.5" ]; then
    ERRORS+=("ACE-Step-1.5/ source directory not found in $SCRIPT_DIR")
fi

# --- Print warnings ---
if [ ${#WARNINGS[@]} -gt 0 ]; then
    echo ""
    for w in "${WARNINGS[@]}"; do
        echo "  [WARN] $w"
    done
fi

# --- Abort on errors ---
if [ ${#ERRORS[@]} -gt 0 ]; then
    echo ""
    echo "=========================================="
    echo "  Missing dependencies - cannot proceed:"
    echo "=========================================="
    for e in "${ERRORS[@]}"; do
        echo "  [MISSING] $e"
    done
    echo ""
    echo "Install the above, then re-run this script."
    exit 1
fi

echo ""
echo "All required dependencies found. Proceeding with install..."
echo ""

# ============================================================
#  Phase 2: Conda environment
# ============================================================
if ! conda env list 2>/dev/null | grep -q "^${CONDA_ENV_NAME}[[:space:]]"; then
    echo "[1/4] Creating conda environment '$CONDA_ENV_NAME' (Python 3.12)..."
    $CONDA_CMD create -y -n "$CONDA_ENV_NAME" python=3.12
    echo "[OK] Conda env created"
else
    echo "[1/4] Conda env '$CONDA_ENV_NAME' already exists"
fi

PYTHON="$(conda run -n "$CONDA_ENV_NAME" which python)"

# ============================================================
#  Phase 3: Python packages
# ============================================================
echo "[2/4] Installing Python packages..."

# Detect CUDA version for PyTorch index
if [ -n "$CUDA_VERSION_DETECTED" ]; then
    CUDA_MAJOR="${CUDA_VERSION_DETECTED%%.*}"
    CUDA_MINOR="$(echo "$CUDA_VERSION_DETECTED" | cut -d. -f2)"
    CUDA_NUM="${CUDA_MAJOR}${CUDA_MINOR}"
    if   [ "$CUDA_NUM" -ge 128 ]; then TORCH_INDEX="cu128"
    elif [ "$CUDA_NUM" -ge 126 ]; then TORCH_INDEX="cu126"
    elif [ "$CUDA_NUM" -ge 118 ]; then TORCH_INDEX="cu118"
    else TORCH_INDEX="cpu"
    fi
else
    TORCH_INDEX="cpu"
fi

echo "  PyTorch index: $TORCH_INDEX"

conda run -n "$CONDA_ENV_NAME" pip install --upgrade pip --quiet

conda run -n "$CONDA_ENV_NAME" pip install \
    torch==2.7.1 torchaudio==2.7.1 torchvision \
    --index-url "https://download.pytorch.org/whl/$TORCH_INDEX"

conda run -n "$CONDA_ENV_NAME" pip install hatchling editables

conda run -n "$CONDA_ENV_NAME" pip install \
    -e ACE-Step-1.5/acestep/third_parts/nano-vllm/ --quiet

conda run -n "$CONDA_ENV_NAME" pip install \
    "transformers>=4.51.0,<4.58.0" diffusers gradio==6.2.0 \
    matplotlib scipy soundfile loguru einops accelerate fastapi diskcache \
    "uvicorn[standard]" numba vector-quantize-pytorch torchcodec \
    "torchao>=0.16.0,<0.17.0" toml peft modelscope tensorboard \
    typer-slim hf_transfer hf_xet lightning lycoris-lora safetensors xxhash

if [ "$TORCH_INDEX" != "cpu" ]; then
    conda run -n "$CONDA_ENV_NAME" pip install "triton>=3.0.0,<3.4" || \
        echo "  [WARN] triton install failed - torch.compile will be unavailable"
fi

conda run -n "$CONDA_ENV_NAME" pip install -e ACE-Step-1.5/ --no-deps

echo "[OK] Python packages installed"

# ============================================================
#  Phase 4: npm dependencies + frontend build
# ============================================================
echo "[3/4] Installing npm dependencies..."

mkdir -p app/data app/server/public/audio

cd "$SCRIPT_DIR/app"
npm install

cd "$SCRIPT_DIR/app/server"
npm install

echo "[4/4] Building frontend..."
cd "$SCRIPT_DIR/app"
npx vite build

# ============================================================
#  Save config
# ============================================================
cd "$SCRIPT_DIR"
echo "$TORCH_INDEX" > cuda_version.txt
echo "conda:$CONDA_ENV_NAME" > python_backend.txt

echo ""
echo "========================================"
echo "  Install complete!"
echo ""
echo "  Conda env : $CONDA_ENV_NAME"
echo "  PyTorch   : $TORCH_INDEX"
echo ""
echo "  To start  : conda run -n $CONDA_ENV_NAME bash run.sh"
echo "  Or activate first:"
echo "    conda activate $CONDA_ENV_NAME && ./run.sh"
echo "========================================"
